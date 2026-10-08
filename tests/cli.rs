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

    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
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

    let report = probe(root, &["v1", "v2"]);
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
