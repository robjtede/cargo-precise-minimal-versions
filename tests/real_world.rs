use std::{fs, process::Command};

use serde_json::{Value, json};
use tempfile::TempDir;

const ROXMLTREE_REVISION: &str = "3f5c78dea57662b43a2f41961397b1f5ffe27166";

fn success(command: &mut Command) -> std::process::Output {
    let output = command.output().unwrap();

    assert!(
        output.status.success(),
        "{command:?}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    output
}

#[test]
#[ignore = "needs GitHub, crates.io, nightly Cargo, and Rust 1.88.0"]
fn roxmltree_requires_memchr_2_1_0() {
    let temp = TempDir::new().unwrap();
    let checkout = temp.path().join("roxmltree");

    success(
        Command::new("git")
            .args([
                "clone",
                "--quiet",
                "--no-checkout",
                "https://github.com/RazrFalcon/roxmltree.git",
            ])
            .arg(&checkout),
    );
    success(Command::new("git").arg("-C").arg(&checkout).args([
        "checkout",
        "--quiet",
        ROXMLTREE_REVISION,
    ]));

    let manifest = checkout.join("Cargo.toml");
    let original_manifest = fs::read(&manifest).unwrap();
    let lockfile = checkout.join("Cargo.lock");
    let original_lockfile = fs::read(&lockfile).ok();

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-precise-minimal-versions"))
        .arg("--manifest-path")
        .arg(&manifest)
        .args([
            "--dependency",
            "memchr",
            "--no-default-features",
            "--rustc-toolchain",
            "1.88.0",
            "--search",
            "linear",
            "--json",
            "--check",
        ])
        .arg("--report-dir")
        .arg(temp.path().join("report"))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let probes = report["probes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|probe| json!([probe["version"], probe["outcome"]]))
        .collect::<Vec<_>>();

    assert_eq!(report["declared_requirement"], "^2", "{report:#}");
    assert_eq!(report["exhaustive"], true, "{report:#}");
    assert_eq!(report["minimum_verified"], true, "{report:#}");
    assert_eq!(report["lowest_passing"], "2.1.0", "{report:#}");
    assert_eq!(report["recommendation"], "2.1.0", "{report:#}");
    assert_eq!(
        probes,
        [
            json!(["2.0.0", "api_failure"]),
            json!(["2.0.1", "api_failure"]),
            json!(["2.0.2", "api_failure"]),
            json!(["2.1.0", "passed"]),
        ],
        "{report:#}"
    );
    assert_eq!(fs::read(manifest).unwrap(), original_manifest);
    assert_eq!(fs::read(lockfile).ok(), original_lockfile);
}
