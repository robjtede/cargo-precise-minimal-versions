use std::{fs, path::Path, process::Command};

use serde_json::Value;
use tempfile::TempDir;

fn package(root: &Path, name: &str, version: &str, dependencies: &str, source: &str) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = {name:?}\nversion = {version:?}\nedition = \"2021\"\n\n[workspace]\n\n[dependencies]\n{dependencies}\n"
        ),
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), source).unwrap();
}

#[test]
fn direct_floor_is_not_raised_by_another_dependency() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    package(&root.join("v1"), "widget", "1.0.0", "", "pub fn old() {}\n");
    package(
        &root.join("v11"),
        "widget",
        "1.1.0",
        "",
        "pub fn needed() {}\n",
    );
    package(
        &root.join("v12"),
        "widget",
        "1.2.0",
        "",
        "pub fn needed() {}\n",
    );
    package(
        &root.join("bridge"),
        "bridge",
        "0.1.0",
        "widget = { version = \"1.2\", path = \"../v12\" }",
        "pub fn call() { widget::needed(); }\n",
    );
    package(
        &root.join("library"),
        "library",
        "0.1.0",
        "widget = { version = \"1\", path = \"../v12\" }\nbridge = { path = \"../bridge\" }",
        "pub fn call() { widget::needed(); bridge::call(); }\n",
    );

    let manifest = root.join("library/Cargo.toml");
    let original = fs::read(&manifest).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-precise-minimal-versions"))
        .env("CARGO_BUILD_WARNINGS", "deny")
        .arg("precise-minimal-versions")
        .arg("--manifest-path")
        .arg(&manifest)
        .args(["--dependency", "widget", "--offline", "--json"])
        .arg("--candidate-path")
        .arg(root.join("v1"))
        .arg("--candidate-path")
        .arg(root.join("v11"))
        .arg("--candidate-path")
        .arg(root.join("v12"))
        .arg("--report-dir")
        .arg(root.join("report"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    if report["lowest_passing"].is_null() {
        for probe in report["probes"].as_array().unwrap() {
            eprintln!(
                "{}",
                fs::read_to_string(probe["diagnostics"].as_str().unwrap()).unwrap()
            );
        }
    }
    assert_eq!(report["lowest_passing"], "1.1.0");
    assert_eq!(report["resolved_version"], "1.2.0");
    assert_eq!(report["probes"][0]["outcome"], "api_failure");
    assert_eq!(report["exhaustive"], false);
    assert!(report["recommendation"].is_null());
    assert_eq!(fs::read(manifest).unwrap(), original);
    assert!(!root.join("library/Cargo.lock").exists());
}

fn probe(root: &Path, candidates: &[&str]) -> Value {
    probe_with_args(root, candidates, &[])
}

fn probe_with_args(root: &Path, candidates: &[&str], args: &[&str]) -> Value {
    let output = probe_output(root, candidates, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn probe_output(root: &Path, candidates: &[&str], args: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cargo-precise-minimal-versions"));
    command
        .arg("--manifest-path")
        .arg(root.join("library/Cargo.toml"))
        .args(["--dependency", "widget", "--offline", "--json"])
        .arg("--report-dir")
        .arg(root.join("report"));

    for candidate in candidates {
        command.arg("--candidate-path").arg(root.join(candidate));
    }

    command.args(args);

    command.output().unwrap()
}

#[test]
fn shared_type_conflicts_do_not_raise_the_recommendation() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    package(
        &root.join("v1"),
        "widget",
        "1.0.0",
        "",
        "pub struct Shared;",
    );
    package(
        &root.join("v2"),
        "widget",
        "1.2.0",
        "",
        "pub struct Shared;",
    );
    package(
        &root.join("bridge"),
        "bridge",
        "0.1.0",
        "widget = { version = \"1.2\", path = \"../v2\" }",
        "pub fn value() -> widget::Shared { widget::Shared }",
    );
    package(
        &root.join("library"),
        "library",
        "0.1.0",
        "widget = { version = \"1\", path = \"../v2\" }\nbridge = { path = \"../bridge\" }",
        "pub fn value() -> widget::Shared { bridge::value() }",
    );

    let report = probe_with_args(root, &["v1", "v2"], &["--check"]);
    assert_eq!(report["probes"][0]["outcome"], "inconclusive");
    assert!(report["recommendation"].is_null());
}

#[test]
fn probes_preserve_build_script_environment_and_existing_lockfile() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    package(&root.join("v1"), "widget", "1.0.0", "", "pub fn call() {}");
    package(
        &root.join("library"),
        "library",
        "0.1.0",
        "widget = { version = \"1\", path = \"../v1\" }",
        "pub fn call() -> &'static str { widget::call(); env!(\"CPMV_FIXTURE_VALUE\") }",
    );
    fs::write(
        root.join("library/build.rs"),
        "fn main() { println!(\"cargo:rustc-env=CPMV_FIXTURE_VALUE=fixture\"); }",
    )
    .unwrap();
    let lockfile = "# Deliberately unused lockfile; the tool must not read or modify it.\n";
    fs::write(root.join("library/Cargo.lock"), lockfile).unwrap();

    let report = probe(root, &["v1"]);
    assert_eq!(report["lowest_passing"], "1.0.0", "{report:#}");
    assert_eq!(
        fs::read_to_string(root.join("library/Cargo.lock")).unwrap(),
        lockfile
    );
}

fn feature_fixture(root: &Path, root_features: &str) {
    package(
        &root.join("v1"),
        "widget",
        "1.0.0",
        "\n[features]\nextra = []",
        "#[cfg(feature = \"extra\")] pub fn needed() {}",
    );
    package(
        &root.join("v2"),
        "widget",
        "1.1.0",
        "\n[features]\nextra = []",
        "pub fn needed() {}",
    );
    package(
        &root.join("bridge"),
        "bridge",
        "0.1.0",
        "widget = { version = \"1.1\", path = \"../v2\", features = [\"extra\"] }",
        "pub fn call() { widget::needed(); }",
    );
    package(
        &root.join("library"),
        "library",
        "0.1.0",
        &format!(
            "widget = {{ version = \"1\", path = \"../v2\", default-features = false }}\nbridge = {{ path = \"../bridge\" }}\n{root_features}"
        ),
        "pub fn call() { widget::needed(); bridge::call(); }",
    );
}

#[test]
fn other_packages_features_do_not_mask_missing_direct_apis() {
    let temp = TempDir::new().unwrap();
    feature_fixture(temp.path(), "");
    let report = probe(temp.path(), &["v1", "v2"]);

    assert_eq!(report["lowest_passing"], "1.1.0");
    assert_eq!(report["candidate_features"], serde_json::json!([]));
}

#[test]
fn root_feature_forwarding_is_preserved() {
    let temp = TempDir::new().unwrap();
    feature_fixture(
        temp.path(),
        "[features]\ndefault = [\"enable\"]\nenable = [\"widget/extra\"]",
    );
    let report = probe(temp.path(), &["v1", "v2"]);

    assert_eq!(report["lowest_passing"], "1.0.0");
    assert_eq!(report["candidate_features"], serde_json::json!(["extra"]));
}

#[test]
fn explicit_package_features_are_forwarded() {
    let temp = TempDir::new().unwrap();
    feature_fixture(temp.path(), "[features]\nenable = [\"widget/extra\"]");
    let report = probe_with_args(
        temp.path(),
        &["v1", "v2"],
        &["--features", "library/enable"],
    );

    assert_eq!(report["lowest_passing"], "1.0.0");
    assert_eq!(report["candidate_features"], serde_json::json!(["extra"]));
}

#[test]
fn missing_direct_features_are_verified_at_the_resolved_version() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    package(
        &root.join("v1"),
        "widget",
        "1.0.0",
        "\n[features]\nextra = []",
        "#[cfg(feature = \"extra\")] pub fn needed() {}",
    );
    package(
        &root.join("bridge"),
        "bridge",
        "0.1.0",
        "widget = { version = \"1\", path = \"../v1\", features = [\"extra\"] }",
        "pub fn call() { widget::needed(); }",
    );
    package(
        &root.join("library"),
        "library",
        "0.1.0",
        "widget = { version = \"1\", path = \"../v1\", default-features = false }\nbridge = { path = \"../bridge\" }",
        "pub fn call() { widget::needed(); bridge::call(); }",
    );

    let report = probe(root, &["v1"]);

    assert_eq!(report["feature_check"]["probe"]["version"], "1.0.0");
    assert_eq!(
        report["feature_check"]["missing_features"],
        serde_json::json!(["extra"]),
        "{}",
        fs::read_to_string(
            report["feature_check"]["probe"]["diagnostics"]
                .as_str()
                .unwrap()
        )
        .unwrap()
    );
    assert_eq!(
        report["feature_check"]["recommended_features"],
        serde_json::json!(["extra"])
    );
    assert_eq!(report["feature_check"]["recheck"]["outcome"], "passed");
    assert_eq!(report["probes"][0]["outcome"], "inconclusive");
    assert!(report["recommendation"].is_null());
}

#[test]
fn verified_feature_suggestions_are_printed_in_terminal_output() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    feature_fixture(root, "");
    fs::write(
        root.join("v2/src/lib.rs"),
        "#[cfg(feature = \"extra\")] pub fn needed() {}",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-precise-minimal-versions"))
        .arg("--manifest-path")
        .arg(root.join("library/Cargo.toml"))
        .args(["--dependency", "widget", "--offline"])
        .arg("--candidate-path")
        .arg(root.join("v2"))
        .arg("--report-dir")
        .arg(root.join("report"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line == "Suggested direct feature: widget/extra"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn check_mode_saves_and_prints_verified_findings_before_failing() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    feature_fixture(root, "");
    fs::write(
        root.join("v2/src/lib.rs"),
        "#[cfg(feature = \"extra\")] pub fn needed() {}",
    )
    .unwrap();

    let output = probe_output(root, &["v2"], &["--check"]);

    assert_eq!(output.status.code(), Some(3), "{output:?}");

    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let saved: Value =
        serde_json::from_slice(&fs::read(root.join("report/report.json")).unwrap()).unwrap();

    assert_eq!(report, saved);
    assert_eq!(
        report["feature_check"]["recommended_features"],
        serde_json::json!(["extra"])
    );
}

#[test]
fn check_mode_accepts_a_passing_sample_without_claiming_a_verified_minimum() {
    let temp = TempDir::new().unwrap();
    feature_fixture(
        temp.path(),
        "[features]\ndefault = [\"enable\"]\nenable = [\"widget/extra\"]",
    );

    let report = probe_with_args(temp.path(), &["v1", "v2"], &["--check"]);

    assert_eq!(report["lowest_passing"], "1.0.0");
    assert_eq!(report["minimum_verified"], false);
    assert!(report["recommendation"].is_null());
}

#[test]
fn check_mode_keeps_operational_failures_distinct_from_findings() {
    let temp = TempDir::new().unwrap();
    let output = probe_output(temp.path(), &[], &["--check"]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("read manifest path"));
}

#[test]
fn report_directories_create_parents_but_do_not_overwrite_existing_reports() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    feature_fixture(root, "");
    let report_dir = root.join("artifacts/dependencies/widget");
    let mut command = Command::new(env!("CARGO_BIN_EXE_cargo-precise-minimal-versions"));
    command
        .arg("--manifest-path")
        .arg(root.join("library/Cargo.toml"))
        .args(["--dependency", "widget", "--offline", "--check"])
        .arg("--candidate-path")
        .arg(root.join("v2"))
        .arg("--report-dir")
        .arg(&report_dir);

    let output = command.output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let original = fs::read(report_dir.join("report.json")).unwrap();

    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(fs::read(report_dir.join("report.json")).unwrap(), original);
}

#[test]
fn feature_fixes_can_be_verified_while_shared_types_remain_inconclusive() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    package(
        &root.join("v1"),
        "widget",
        "1.0.0",
        "\n[features]\nextra = []\nunrelated = []",
        "#[cfg(feature = \"extra\")] pub fn needed() {}\npub struct Shared;",
    );
    package(
        &root.join("bridge"),
        "bridge",
        "0.1.0",
        "widget = { version = \"1\", path = \"../v1\", features = [\"extra\", \"unrelated\"] }",
        "pub fn value() -> widget::Shared { widget::Shared }",
    );
    package(
        &root.join("library"),
        "library",
        "0.1.0",
        "widget = { version = \"1\", path = \"../v1\", default-features = false }\nbridge = { path = \"../bridge\" }",
        "pub fn call() -> widget::Shared { widget::needed(); bridge::value() }",
    );

    let report = probe(root, &["v1"]);

    assert_eq!(
        report["feature_check"]["recommended_features"],
        serde_json::json!(["extra"])
    );
    assert_eq!(
        report["feature_check"]["recheck"]["outcome"],
        "inconclusive"
    );
    assert_eq!(report["minimum_verified"], false);
    assert!(report["recommendation"].is_null());
}

#[test]
fn transitive_feature_gates_are_not_assigned_to_the_direct_dependency() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    package(
        &root.join("leaf"),
        "leaf",
        "1.0.0",
        "\n[features]\nextra = []",
        "#[cfg(feature = \"extra\")] pub fn needed() {}",
    );
    package(
        &root.join("v1"),
        "widget",
        "1.0.0",
        "leaf = { path = \"../leaf\", default-features = false }\n\n[features]\nextra = []",
        "pub use leaf as api;",
    );
    package(
        &root.join("bridge"),
        "bridge",
        "0.1.0",
        "leaf = { path = \"../leaf\", features = [\"extra\"] }",
        "pub fn call() { leaf::needed(); }",
    );
    package(
        &root.join("library"),
        "library",
        "0.1.0",
        "widget = { version = \"1\", path = \"../v1\" }\nbridge = { path = \"../bridge\" }",
        "pub fn call() { widget::api::needed(); bridge::call(); }",
    );

    let report = probe(root, &["v1"]);

    assert_eq!(
        report["feature_check"]["missing_features"],
        serde_json::json!([])
    );
    assert_eq!(
        report["feature_check"]["recommended_features"],
        serde_json::json!([])
    );
    assert!(report["feature_check"]["recheck"].is_null());
    assert_eq!(report["probes"][0]["outcome"], "inconclusive");
}

#[test]
fn a_failed_feature_recheck_does_not_recommend_the_feature() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    package(
        &root.join("v1"),
        "widget",
        "1.0.0",
        "\n[features]\nextra = []\nbaseline = []",
        "#[cfg(all(feature = \"extra\", not(feature = \"baseline\")))]\ncompile_error!(\"extra requires baseline\");\n#[cfg(feature = \"extra\")] pub fn needed() {}",
    );
    package(
        &root.join("bridge"),
        "bridge",
        "0.1.0",
        "widget = { version = \"1\", path = \"../v1\", features = [\"extra\", \"baseline\"] }",
        "pub fn call() { widget::needed(); }",
    );
    package(
        &root.join("library"),
        "library",
        "0.1.0",
        "widget = { version = \"1\", path = \"../v1\", default-features = false }\nbridge = { path = \"../bridge\" }",
        "pub fn call() { widget::needed(); bridge::call(); }",
    );

    let report = probe_with_args(root, &["v1"], &["--check"]);

    assert_eq!(
        report["feature_check"]["missing_features"],
        serde_json::json!(["extra"])
    );
    assert_eq!(
        report["feature_check"]["recommended_features"],
        serde_json::json!([])
    );
    assert_eq!(
        report["feature_check"]["recheck"]["outcome"],
        "candidate_build_failed"
    );
}

#[test]
fn candidate_msrv_is_reported_without_treating_it_as_an_api_failure() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    package(&root.join("v1"), "widget", "1.0.0", "", "pub fn call() {}");
    package(&root.join("v2"), "widget", "1.1.0", "", "pub fn call() {}");
    let path = root.join("v1/Cargo.toml");
    let manifest = fs::read_to_string(&path)
        .unwrap()
        .replace("[package]", "[package]\nrust-version = \"99.0\"");
    fs::write(path, manifest).unwrap();
    package(
        &root.join("library"),
        "library",
        "0.1.0",
        "widget = { version = \"1\", path = \"../v2\" }",
        "pub fn call() { widget::call(); }",
    );

    let report = probe(root, &["v1", "v2"]);
    assert_eq!(report["probes"][0]["outcome"], "requires_newer_rust");
    assert_eq!(report["lowest_passing"], "1.1.0");
    assert!(report["recommendation"].is_null());
}

fn version_range_fixture(root: &Path) -> Vec<String> {
    let candidates = (0..32).map(|index| format!("v{index}")).collect::<Vec<_>>();

    for (index, path) in candidates.iter().enumerate() {
        package(
            &root.join(path),
            "widget",
            &format!("1.0.{index}"),
            "",
            if index < 23 {
                "pub fn old() {}"
            } else {
                "pub fn needed() {}"
            },
        );
    }

    package(
        &root.join("library"),
        "library",
        "0.1.0",
        "widget = { version = \"1\", path = \"../v31\" }",
        "pub fn call() { widget::needed(); }",
    );

    candidates
}

#[test]
fn large_ranges_use_bisection_without_claiming_a_verified_minimum() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let candidates = version_range_fixture(root);
    let paths = candidates.iter().map(String::as_str).collect::<Vec<_>>();
    let report = probe(root, &paths);

    assert_eq!(report["search"], "bisect");
    assert_eq!(report["bisect_threshold"], 10);
    assert_eq!(report["lowest_passing"], "1.0.23", "{report:#}");
    assert!(
        report["probes"].as_array().unwrap().len() <= 13,
        "{report:#}"
    );
    assert_eq!(report["minimum_verified"], false);
    assert!(report["recommendation"].is_null());
}

#[test]
fn linear_search_checks_all_earlier_candidates() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let candidates = version_range_fixture(root);
    let paths = candidates.iter().map(String::as_str).collect::<Vec<_>>();
    let report = probe_with_args(root, &paths, &["--search", "linear"]);

    assert_eq!(report["search"], "linear");
    assert_eq!(report["lowest_passing"], "1.0.23", "{report:#}");
    assert_eq!(report["probes"].as_array().unwrap().len(), 24);

    for (index, probe) in report["probes"].as_array().unwrap().iter().enumerate() {
        assert_eq!(probe["version"], format!("1.0.{index}"));
    }
}

#[test]
fn bisection_threshold_controls_the_final_scan() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let candidates = version_range_fixture(root);
    let paths = candidates.iter().map(String::as_str).collect::<Vec<_>>();
    let report = probe_with_args(root, &paths, &["--bisect-threshold", "2"]);

    assert_eq!(report["bisect_threshold"], 2);
    assert_eq!(report["lowest_passing"], "1.0.23", "{report:#}");
    assert!(
        report["probes"].as_array().unwrap().len() <= 7,
        "{report:#}"
    );
}

#[test]
fn a_zero_bisection_threshold_is_rejected() {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-precise-minimal-versions"))
        .args(["--dependency", "widget", "--bisect-threshold", "0"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--bisect-threshold"));
}
