use std::{fs, path::Path, process::Command};

use semver::{Version, VersionReq};
use serde_json::{Value, json};
use tempfile::TempDir;

fn copy_fixture(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();

    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();

        if entry.file_name() == "target" {
            continue;
        }

        let destination = destination.join(entry.file_name());

        if entry.file_type().unwrap().is_dir() {
            copy_fixture(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).unwrap();
        }
    }
}

fn run_fixture(name: &str, manifest_name: &str, expected_exit: i32) -> (TempDir, Value) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("project");

    copy_fixture(&source, &root);
    fs::copy(source.join(manifest_name), root.join("Cargo.toml")).unwrap();

    let original_manifest = fs::read(root.join("Cargo.toml")).unwrap();
    let original_lockfile = fs::read(root.join("Cargo.lock")).ok();
    let manifest =
        toml::from_str::<toml::Value>(&fs::read_to_string(root.join("Cargo.toml")).unwrap())
            .unwrap();
    let requirement = VersionReq::parse(
        manifest["dependencies"]["widget"]["version"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_cargo-precise-minimal-versions"));
    command
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .args([
            "--dependency",
            "widget",
            "--search",
            "linear",
            "--offline",
            "--json",
            "--check",
        ])
        .arg("--report-dir")
        .arg(temp.path().join("report"));

    for candidate in ["candidate", "baseline"] {
        let path = root.join(candidate);
        let manifest =
            toml::from_str::<toml::Value>(&fs::read_to_string(path.join("Cargo.toml")).unwrap())
                .unwrap();
        let version = Version::parse(manifest["package"]["version"].as_str().unwrap()).unwrap();

        if requirement.matches(&version) {
            command.arg("--candidate-path").arg(path);
        }
    }

    let output = command.output().unwrap();

    assert_eq!(
        output.status.code(),
        Some(expected_exit),
        "{name}/{manifest_name}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report = serde_json::from_slice::<Value>(&output.stdout).unwrap();

    assert_eq!(report["package"], name);
    assert_eq!(report["exhaustive"], false);
    assert_eq!(report["minimum_verified"], false);
    assert!(report["recommendation"].is_null());
    assert_eq!(
        fs::read(root.join("Cargo.toml")).unwrap(),
        original_manifest
    );
    assert_eq!(fs::read(root.join("Cargo.lock")).ok(), original_lockfile);

    let saved =
        serde_json::from_slice::<Value>(&fs::read(temp.path().join("report/report.json")).unwrap())
            .unwrap();

    assert_eq!(saved, report);

    (temp, report)
}

#[test]
fn api_failure_fixture_finds_the_first_passing_sample() {
    let (_temp, report) = run_fixture("api-failure", "Cargo.toml", 0);

    assert_eq!(report["probes"][0]["outcome"], "api_failure");
    assert_eq!(report["probes"][1]["outcome"], "passed");
    assert_eq!(report["lowest_passing"], "1.1.0");
    assert_eq!(report["feature_check"]["probe"]["outcome"], "passed");
}

#[test]
fn suggested_api_fixture_excludes_the_version_without_the_required_api() {
    let (_temp, report) = run_fixture("api-failure", "Cargo.toml.suggested", 0);

    assert_eq!(report["declared_requirement"], "^1.1.0");
    assert_eq!(report["probes"].as_array().unwrap().len(), 1);
    assert_eq!(report["probes"][0]["version"], "1.1.0");
    assert_eq!(report["probes"][0]["outcome"], "passed");
}

#[test]
fn missing_feature_fixture_verifies_the_feature_and_fails_check_mode() {
    let (_temp, report) = run_fixture("missing-feature", "Cargo.toml", 3);
    let check = &report["feature_check"];

    assert_eq!(check["probe"]["outcome"], "inconclusive");
    assert_eq!(check["missing_features"], json!(["extra"]));
    assert_eq!(check["recommended_features"], json!(["extra"]));
    assert_eq!(check["recheck"]["outcome"], "passed");
    assert!(report["lowest_passing"].is_null());
}

#[test]
fn suggested_missing_feature_fixture_passes_without_a_feature_recommendation() {
    let (_temp, report) = run_fixture("missing-feature", "Cargo.toml.suggested", 0);

    assert_eq!(report["candidate_features"], json!(["extra"]));
    assert_eq!(report["feature_check"]["probe"]["outcome"], "passed");
    assert_eq!(report["feature_check"]["recommended_features"], json!([]));
    assert!(report["feature_check"]["recheck"].is_null());
    assert_eq!(report["probes"][0]["version"], "1.0.0");
    assert_eq!(report["probes"][0]["outcome"], "passed");
}

#[test]
fn shared_types_fixture_keeps_type_conflicts_inconclusive() {
    let (_temp, report) = run_fixture("shared-types", "Cargo.toml", 0);
    let probe = &report["probes"][0];
    let diagnostics = fs::read_to_string(probe["diagnostics"].as_str().unwrap()).unwrap();

    assert_eq!(probe["outcome"], "inconclusive");
    assert!(diagnostics.contains("E0308"), "{diagnostics}");
    assert_eq!(report["feature_check"]["recommended_features"], json!([]));
}

#[test]
fn suggested_shared_types_fixture_keeps_the_requirement_and_remains_inconclusive() {
    let (_temp, report) = run_fixture("shared-types", "Cargo.toml.suggested", 0);

    assert_eq!(report["declared_requirement"], "^1");
    assert_eq!(report["probes"].as_array().unwrap().len(), 2);
    assert!(
        report["probes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|probe| probe["outcome"] == "inconclusive")
    );
    assert_eq!(report["feature_check"]["recommended_features"], json!([]));
    assert!(report["lowest_passing"].is_null());
}

#[test]
fn candidate_build_fixture_keeps_build_errors_distinct_from_missing_apis() {
    let (_temp, report) = run_fixture("candidate-build-failed", "Cargo.toml", 0);
    let probe = &report["probes"][0];
    let diagnostics = Path::new(probe["diagnostics"].as_str().unwrap()).with_extension("stdout");
    let diagnostics = fs::read_to_string(diagnostics).unwrap();

    assert_eq!(probe["outcome"], "candidate_build_failed");
    assert!(
        diagnostics.contains("This fixture makes the isolated candidate build fail."),
        "{diagnostics}"
    );
    assert_eq!(report["probes"][1]["outcome"], "passed");
}

#[test]
fn suggested_candidate_build_fixture_excludes_the_broken_candidate() {
    let (_temp, report) = run_fixture("candidate-build-failed", "Cargo.toml.suggested", 0);

    assert_eq!(report["declared_requirement"], "^1.1.0");
    assert_eq!(report["probes"].as_array().unwrap().len(), 1);
    assert_eq!(report["probes"][0]["version"], "1.1.0");
    assert_eq!(report["probes"][0]["outcome"], "passed");
}

#[test]
fn newer_rust_fixture_reports_the_candidate_compiler_requirement() {
    let (_temp, report) = run_fixture("requires-newer-rust", "Cargo.toml", 0);

    assert_eq!(report["probes"][0]["outcome"], "requires_newer_rust");
    assert_eq!(report["probes"][1]["outcome"], "passed");
    assert_eq!(report["lowest_passing"], "1.1.0");
}

#[test]
fn suggested_newer_rust_fixture_excludes_the_unavailable_compiler_requirement() {
    let (_temp, report) = run_fixture("requires-newer-rust", "Cargo.toml.suggested", 0);

    assert_eq!(report["declared_requirement"], "^1.1.0");
    assert_eq!(report["probes"].as_array().unwrap().len(), 1);
    assert_eq!(report["probes"][0]["version"], "1.1.0");
    assert_eq!(report["probes"][0]["outcome"], "passed");
}
