use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use anyhow::{Context, Result, ensure};
use semver::{Op, Version, VersionReq};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    Args,
    candidates::Candidate,
    features::{self, FeatureCheck},
    probe::{self, Invocation, Outcome, Probe},
};

#[derive(Deserialize)]
pub struct Package {
    id: String,
    pub name: String,
    manifest_path: PathBuf,
    rust_version: Option<String>,
    dependencies: Vec<Dependency>,
    features: BTreeMap<String, Vec<String>>,
    targets: Vec<Target>,
}

#[derive(Deserialize)]
struct Target {
    name: String,
    kind: Vec<String>,
}

#[derive(Deserialize)]
pub struct Dependency {
    pub name: String,
    pub req: VersionReq,
    pub source: Option<String>,
    rename: Option<String>,
    kind: Option<String>,
    target: Option<String>,
    features: Vec<String>,
    uses_default_features: bool,
    optional: bool,
}

impl Dependency {
    fn key(&self) -> &str {
        self.rename.as_deref().unwrap_or(&self.name)
    }
}

impl Package {
    pub fn selectable_dependencies(&self, args: &Args) -> Vec<String> {
        self.dependencies
            .iter()
            .filter(|dep| {
                self.dependency(dep.key()).is_ok()
                    && self.dependency_features(args, dep).is_ok()
                    && dep.kind.is_none()
            })
            .map(|dep| dep.key().to_owned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn dependency(&self, key: &str) -> Result<&Dependency> {
        let matches = self
            .dependencies
            .iter()
            .filter(|dep| dep.key() == key && dep.kind.is_none())
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "expected one normal dependency named {key}; target-specific duplicate declarations are not supported"
        );
        let dependency = matches[0];
        ensure!(
            dependency.target.is_none(),
            "target-specific dependencies are not supported yet"
        );
        ensure!(
            dependency.req.comparators.len() == 1 && dependency.req.comparators[0].op == Op::Caret,
            "only caret requirements are supported; found {}",
            dependency.req
        );
        Ok(dependency)
    }

    pub fn dependency_features(&self, args: &Args, dependency: &Dependency) -> Result<Vec<String>> {
        let mut enabled = BTreeSet::new();
        let qualifier = format!("{}/", self.name);
        let mut pending = args
            .features
            .iter()
            .flat_map(|features| features.split_whitespace())
            .map(|feature| {
                feature
                    .strip_prefix(&qualifier)
                    .unwrap_or(feature)
                    .to_owned()
            })
            .collect::<Vec<_>>();

        if !args.no_default_features && self.features.contains_key("default") {
            pending.push("default".into());
        }

        while let Some(feature) = pending.pop() {
            if enabled.insert(feature.clone())
                && let Some(children) = self.features.get(&feature)
            {
                pending.extend(children.iter().cloned());
            }
        }

        let key = dependency.key();
        let active = !dependency.optional
            || enabled.contains(&format!("dep:{key}"))
            || enabled
                .iter()
                .any(|feature| feature.starts_with(&format!("{key}/")));
        ensure!(
            active,
            "dependency {key} is inactive in the selected feature configuration"
        );
        let mut result = dependency.features.iter().cloned().collect::<BTreeSet<_>>();

        for feature in &enabled {
            if let Some((name, feature)) = feature.split_once('/')
                && name.trim_end_matches('?') == key
            {
                result.insert(feature.to_owned());
            }
        }

        Ok(result.into_iter().collect())
    }

    fn library(&self) -> Result<&Target> {
        self.targets
            .iter()
            .find(|target| {
                target
                    .kind
                    .iter()
                    .any(|kind| matches!(kind.as_str(), "lib" | "rlib"))
            })
            .context("selected package must have a Rust library target")
    }
}

pub struct Cargo {
    work: PathBuf,
    toolchain: String,
    compiler: PathBuf,
    pub compiler_version: String,
    pub host: String,
    compiler_semver: Version,
    legacy_rustc: bool,
    offline: bool,
}

pub struct Baseline {
    invocation: Invocation,
    pub resolved_version: Version,
    feature_candidate: Candidate,
}

struct ProbeSettings<'a> {
    candidate: &'a Candidate,
    features: &'a [String],
    name: &'a str,
}

impl Cargo {
    pub fn new(args: &Args, work: &Path) -> Result<Self> {
        let output = Command::new("rustup")
            .args(["which", "--toolchain", &args.toolchain, "rustc"])
            .output()?;
        ensure_success(&output, "locate nightly compiler")?;
        let compiler = PathBuf::from(String::from_utf8(output.stdout)?.trim());
        let output = Command::new("cargo")
            .arg(format!("+{}", args.toolchain))
            .arg("--version")
            .output()?;
        ensure_success(&output, "read Cargo version")?;
        ensure!(
            String::from_utf8_lossy(&output.stdout).contains("nightly"),
            "--toolchain must select nightly Cargo for isolated lockfiles"
        );
        let cargo_version = String::from_utf8_lossy(&output.stdout);
        let mut cargo_version = Version::parse(
            cargo_version
                .split_whitespace()
                .nth(1)
                .context("missing Cargo version")?,
        )?;
        cargo_version.pre = semver::Prerelease::EMPTY;
        ensure!(
            cargo_version >= Version::new(1, 97, 0),
            "Cargo 1.97 or newer is required for resolver.lockfile-path; update the nightly toolchain"
        );

        Ok(Self {
            work: work.to_owned(),
            toolchain: args.toolchain.clone(),
            compiler,
            compiler_version: String::new(),
            host: String::new(),
            compiler_semver: Version::new(0, 0, 0),
            legacy_rustc: false,
            offline: args.offline,
        })
    }

    fn command(&self, subcommand: &str) -> Command {
        let mut command = Command::new("cargo");
        command
            .arg(format!("+{}", self.toolchain))
            .arg(subcommand)
            .env("RUSTC", &self.compiler)
            .env(
                probe::LEGACY_RUSTC_ENV,
                if self.legacy_rustc { "1" } else { "0" },
            )
            .env("RUSTC_WRAPPER", "")
            .env("RUSTC_WORKSPACE_WRAPPER", "")
            .env("CARGO_INCREMENTAL", "0")
            .env_remove("CARGO_RESOLVER_LOCKFILE_PATH")
            .env("CARGO_TERM_COLOR", "never");

        if self.offline {
            command.arg("--offline");
        }

        command
    }

    fn root_command(&self, args: &Args, subcommand: &str) -> Command {
        let mut command = self.command(subcommand);
        command
            .arg("--manifest-path")
            .arg(&args.manifest_path)
            .env(
                "CARGO_RESOLVER_LOCKFILE_PATH",
                self.work.join("root/Cargo.lock"),
            )
            .current_dir(args.manifest_path.parent().unwrap_or(Path::new(".")));

        if !args.features.is_empty() {
            command.arg("--features").arg(args.features.join(","));
        }

        if args.no_default_features {
            command.arg("--no-default-features");
        }

        command
    }

    pub fn package(&self, args: &mut Args) -> Result<Package> {
        fs::create_dir_all(self.work.join("root"))?;
        let output = self
            .root_command(args, "metadata")
            .args(["--format-version", "1", "--no-deps"])
            .output()?;
        ensure_success(&output, "read project metadata")?;
        let value: Value = serde_json::from_slice(&output.stdout)?;
        let packages: Vec<Package> = serde_json::from_value(value["packages"].clone())?;
        let members = value["workspace_members"]
            .as_array()
            .context("missing workspace members")?;
        if args.interactive && args.package.is_none() {
            let mut choices = packages
                .iter()
                .filter(|package| {
                    members.contains(&Value::String(package.id.clone()))
                        && package.library().is_ok()
                })
                .map(|package| package.name.clone())
                .collect::<Vec<_>>();
            choices.sort();
            let index = crate::interactive::select("Select a workspace library", &choices)?;
            args.package = Some(choices[index].clone());
        }
        let package = packages
            .into_iter()
            .find(|package| {
                members.contains(&Value::String(package.id.clone()))
                    && match &args.package {
                        Some(name) => package.name == *name,
                        None => package.manifest_path == args.manifest_path,
                    }
            })
            .context("select a library manifest, or use --package for a workspace member")?;
        package.library()?;
        Ok(package)
    }

    pub fn select_compiler(&mut self, args: &Args, package: &Package) -> Result<()> {
        let msrv = package
            .rust_version
            .as_deref()
            .map(crate::candidates::rust_version)
            .transpose()?
            .map(|version| version.to_string());
        let toolchain = args
            .rustc_toolchain
            .as_deref()
            .or(msrv.as_deref())
            .unwrap_or(&args.toolchain);
        let output = Command::new("rustup")
            .args(["which", "--toolchain", toolchain, "rustc"])
            .output()?;
        ensure_success(
            &output,
            "locate compilation toolchain (install it with rustup, or set --rustc-toolchain)",
        )?;
        self.compiler = PathBuf::from(String::from_utf8(output.stdout)?.trim());
        let output = Command::new(&self.compiler).arg("-vV").output()?;
        ensure_success(&output, "read compiler version")?;
        let version = String::from_utf8(output.stdout)?;
        self.compiler_version = version
            .lines()
            .next()
            .context("missing rustc version")?
            .to_owned();
        self.host = version
            .lines()
            .find_map(|line| line.strip_prefix("host: "))
            .context("missing compiler host")?
            .to_owned();
        self.compiler_semver = Version::parse(
            self.compiler_version
                .split_whitespace()
                .nth(1)
                .context("missing rustc version")?,
        )?;
        self.compiler_semver.pre = semver::Prerelease::EMPTY;

        self.legacy_rustc = !Command::new(&self.compiler)
            .args([
                "--error-format=json",
                "--json=unused-externs-silent",
                "--check-cfg=cfg()",
                "--print=sysroot",
            ])
            .output()
            .context("check compiler diagnostic flag support")?
            .status
            .success();

        Ok(())
    }

    pub fn baseline(
        &self,
        args: &Args,
        package: &Package,
        dependency: &Dependency,
        report: &Path,
    ) -> Result<Baseline> {
        let capture = self.work.join("invocation.json");
        let output = self
            .root_command(args, "build")
            .args(["--lib", "--package", &package.name, "--message-format=json"])
            .args(["--target", &self.host])
            .arg("--target-dir")
            .arg(self.work.join("baseline-target"))
            .env("RUSTC_WRAPPER", env::current_exe()?)
            .env(probe::CAPTURE_ENV, &capture)
            .env(
                "CPMV_ROOT",
                package
                    .manifest_path
                    .parent()
                    .context("missing package directory")?,
            )
            .env("CPMV_CRATE", &package.library()?.name)
            .output()?;
        save_output(report, "baseline", &output)?;
        ensure_success(&output, "baseline build failed; see baseline diagnostics")?;
        let mut invocation = Invocation::read(&capture)?;
        invocation.build_environment(&output.stdout, &package.id)?;
        fs::write(
            report.join("baseline-invocation.json"),
            serde_json::to_vec_pretty(&invocation)?,
        )?;
        let output = self
            .root_command(args, "metadata")
            .args(["--format-version", "1", "--filter-platform", &self.host])
            .output()?;
        ensure_success(&output, "read resolved graph")?;
        fs::write(report.join("baseline-metadata.json"), &output.stdout)?;
        let metadata: Value = serde_json::from_slice(&output.stdout)?;
        let nodes = metadata["resolve"]["nodes"]
            .as_array()
            .context("missing resolved graph")?;
        let node = nodes
            .iter()
            .find(|node| node["id"] == package.id)
            .context("missing selected package in resolved graph")?;
        let edge = node["deps"]
            .as_array()
            .context("missing dependency edges")?
            .iter()
            .find(|edge| edge["name"] == dependency.key().replace('-', "_"))
            .context("missing direct dependency edge")?;
        let resolved = metadata["packages"]
            .as_array()
            .context("missing packages")?
            .iter()
            .find(|package| package["id"] == edge["pkg"])
            .context("missing dependency package")?;
        let resolved_version: Version = serde_json::from_value(resolved["version"].clone())?;
        ensure!(
            !args.candidate_path.is_empty()
                || resolved["source"].as_str() == dependency.source.as_deref(),
            "patched dependency sources require explicit --candidate-path probes"
        );
        Ok(Baseline {
            invocation,
            feature_candidate: Candidate {
                version: resolved_version.clone(),
                path: if resolved["source"].as_str()
                    != Some("registry+https://github.com/rust-lang/crates.io-index")
                {
                    Some(
                        Path::new(
                            resolved["manifest_path"]
                                .as_str()
                                .context("resolved dependency has no manifest path")?,
                        )
                        .parent()
                        .context("resolved dependency manifest has no parent")?
                        .to_owned(),
                    )
                } else {
                    None
                },
                rust_version: None,
            },
            resolved_version,
        })
    }

    pub fn feature_check(
        &self,
        args: &Args,
        dependency: &Dependency,
        requested: &[String],
        baseline: &Baseline,
        report: &Path,
    ) -> Result<FeatureCheck> {
        let probe = self.probe_named(
            args,
            dependency,
            ProbeSettings {
                candidate: &baseline.feature_candidate,
                features: requested,
                name: "feature-check",
            },
            baseline,
            report,
        )?;
        let mut check = FeatureCheck {
            probe,
            missing_features: Vec::new(),
            recommended_features: Vec::new(),
            recheck_features: Vec::new(),
            recheck: None,
        };

        if !matches!(
            check.probe.outcome,
            Outcome::ApiFailure | Outcome::Inconclusive
        ) {
            return Ok(check);
        }

        let metadata: Value =
            serde_json::from_slice(&fs::read(report.join("feature-check-metadata.json"))?)?;
        let package = candidate_package(&metadata)?;
        let enabled = metadata["resolve"]["nodes"]
            .as_array()
            .context("missing candidate graph")?
            .iter()
            .find(|node| node["id"] == package["id"])
            .and_then(|node| node["features"].as_array())
            .context("missing candidate features")?;
        let gates =
            features::missing_features(&fs::read(&check.probe.diagnostics)?, package, enabled)?;
        check.missing_features = gates
            .iter()
            .flat_map(|gate| gate.features.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        if check.missing_features.is_empty() {
            return Ok(check);
        }

        check.recheck_features = requested
            .iter()
            .chain(&check.missing_features)
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let recheck = self.probe_named(
            args,
            dependency,
            ProbeSettings {
                candidate: &baseline.feature_candidate,
                features: &check.recheck_features,
                name: "feature-recheck",
            },
            baseline,
            report,
        )?;

        if matches!(
            recheck.outcome,
            Outcome::Passed | Outcome::ApiFailure | Outcome::Inconclusive
        ) {
            check.recommended_features =
                features::verified_features(&gates, &fs::read(&recheck.diagnostics)?);
        }

        check.recheck = Some(recheck);
        Ok(check)
    }

    pub fn probe(
        &self,
        args: &Args,
        dependency: &Dependency,
        features: &[String],
        candidate: &Candidate,
        baseline: &Baseline,
        report: &Path,
    ) -> Result<Probe> {
        let name = format!("probe-{}", candidate.version);
        self.probe_named(
            args,
            dependency,
            ProbeSettings {
                candidate,
                features,
                name: &name,
            },
            baseline,
            report,
        )
    }

    fn probe_named(
        &self,
        args: &Args,
        dependency: &Dependency,
        settings: ProbeSettings<'_>,
        baseline: &Baseline,
        report: &Path,
    ) -> Result<Probe> {
        let ProbeSettings {
            candidate,
            features,
            name,
        } = settings;
        let diagnostics = report.join(format!("{name}.stderr"));

        if candidate
            .rust_version
            .as_ref()
            .is_some_and(|version| version > &self.compiler_semver)
        {
            fs::write(
                &diagnostics,
                "Candidate declares a newer Rust requirement than the selected compiler.\n",
            )?;
            return Ok(Probe {
                version: candidate.version.clone(),
                outcome: Outcome::RequiresNewerRust,
                diagnostics,
            });
        }

        let project = self.work.join(name);
        fs::create_dir_all(project.join("src"))?;
        let mut spec = json!({
            "package": dependency.name,
            "version": format!("={}", candidate.version),
            "default-features": dependency.uses_default_features,
            "features": features,
        });

        if let Some(path) = &candidate.path {
            spec["path"] = json!(path);
        }

        let manifest = json!({
            "package": {"name": "cpmv-candidate", "version": "0.0.0", "edition": "2015", "rust-version": self.compiler_semver.to_string()},
            "workspace": {},
            "dependencies": {"candidate": spec},
        });
        fs::write(project.join("Cargo.toml"), toml::to_string(&manifest)?)?;
        fs::write(project.join("src/lib.rs"), "pub extern crate candidate;\n")?;
        let capture = project.join("invocation.json");
        let output = self
            .command("build")
            .arg("--manifest-path")
            .arg(project.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(self.work.join("candidate-target"))
            .args(["--target", &self.host])
            .args([
                "--message-format=json",
                "--config",
                "resolver.incompatible-rust-versions=\"fallback\"",
            ])
            .current_dir(
                args.manifest_path
                    .parent()
                    .context("missing manifest directory")?,
            )
            .env("RUSTC_WRAPPER", env::current_exe()?)
            .env(probe::CAPTURE_ENV, &capture)
            .env("CPMV_ROOT", project.canonicalize()?)
            .env("CPMV_CRATE", "cpmv_candidate")
            .output()?;
        save_output(report, &format!("{name}-build"), &output)?;

        if !output.status.success() {
            return Ok(Probe {
                version: candidate.version.clone(),
                outcome: Outcome::CandidateBuildFailed,
                diagnostics: report.join(format!("{name}-build.stderr")),
            });
        }

        let output_metadata = self
            .command("metadata")
            .arg("--manifest-path")
            .arg(project.join("Cargo.toml"))
            .args(["--format-version", "1", "--filter-platform", &self.host])
            .current_dir(
                args.manifest_path
                    .parent()
                    .context("missing manifest directory")?,
            )
            .output()?;
        ensure_success(&output_metadata, "read candidate metadata")?;
        fs::write(
            report.join(format!("{name}-metadata.json")),
            &output_metadata.stdout,
        )?;
        let metadata: Value = serde_json::from_slice(&output_metadata.stdout)?;
        let package = candidate_package(&metadata)?;
        ensure!(
            package["version"] == candidate.version.to_string(),
            "candidate resolved to a different version"
        );
        let candidate_invocation = Invocation::read(&capture)?;
        let artifact = candidate_invocation.dependency("candidate")?;
        ensure!(
            artifact
                .extension()
                .is_some_and(|extension| extension == "rmeta" || extension == "rlib"),
            "candidate must be a Rust library; procedural macros are not supported"
        );
        let output = baseline.invocation.run(
            &args.dependency,
            &artifact,
            &candidate_invocation.search_paths(),
            &project.join("output"),
        )?;
        save_output(report, name, &output)?;
        Ok(Probe {
            version: candidate.version.clone(),
            outcome: probe::classify(&output),
            diagnostics,
        })
    }
}

fn candidate_package(metadata: &Value) -> Result<&Value> {
    let root_id = &metadata["resolve"]["root"];
    let node = metadata["resolve"]["nodes"]
        .as_array()
        .context("missing candidate graph")?
        .iter()
        .find(|node| node["id"] == *root_id)
        .context("missing candidate root")?;
    let edge = node["deps"]
        .as_array()
        .context("missing candidate edges")?
        .iter()
        .find(|edge| edge["name"] == "candidate")
        .context("missing candidate edge")?;

    metadata["packages"]
        .as_array()
        .context("missing candidate packages")?
        .iter()
        .find(|package| package["id"] == edge["pkg"])
        .context("missing candidate package")
}

fn save_output(directory: &Path, name: &str, output: &Output) -> Result<()> {
    fs::write(directory.join(format!("{name}.stdout")), &output.stdout)?;
    fs::write(directory.join(format!("{name}.stderr")), &output.stderr)?;
    Ok(())
}

fn ensure_success(output: &Output, context: &str) -> Result<()> {
    ensure!(
        output.status.success(),
        "{context}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
