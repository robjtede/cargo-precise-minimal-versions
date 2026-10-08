use std::{fs, path::PathBuf};

use anyhow::{Context, Result, ensure};
use semver::Version;
use serde::Deserialize;

use crate::{Args, cargo::Dependency};

pub struct Candidate {
    pub version: Version,
    pub path: Option<PathBuf>,
    pub rust_version: Option<Version>,
}

#[derive(Deserialize)]
struct Release {
    vers: Version,
    #[serde(default)]
    yanked: bool,
    rust_version: Option<String>,
}

pub fn rust_version(value: &str) -> Result<Version> {
    let components = value.split('.').count();
    Version::parse(&format!(
        "{value}{}",
        ".0".repeat(3_usize.saturating_sub(components))
    ))
    .context("invalid rust-version")
}

pub fn list(args: &Args, dependency: &Dependency) -> Result<(Vec<Candidate>, bool)> {
    let exhaustive = args.candidate_path.is_empty() && args.versions.is_empty();
    let mut candidates = Vec::new();

    if !args.candidate_path.is_empty() {
        for path in &args.candidate_path {
            let path = path.canonicalize().context("read candidate directory")?;
            let manifest: toml::Value =
                toml::from_str(&fs::read_to_string(path.join("Cargo.toml"))?)?;
            let package = manifest
                .get("package")
                .context("candidate manifest has no [package] table")?;
            ensure!(
                package.get("name").and_then(toml::Value::as_str) == Some(&dependency.name),
                "candidate package name must be {}",
                dependency.name
            );
            let version = Version::parse(
                package
                    .get("version")
                    .and_then(toml::Value::as_str)
                    .context("candidate needs an explicit package version")?,
            )?;
            ensure!(
                dependency.req.matches(&version),
                "candidate {version} is outside {}",
                dependency.req
            );
            let rust_version = package
                .get("rust-version")
                .and_then(toml::Value::as_str)
                .map(rust_version)
                .transpose()?;
            candidates.push(Candidate {
                version,
                path: Some(path),
                rust_version,
            });
        }
    } else {
        ensure!(
            dependency.source.as_deref()
                == Some("registry+https://github.com/rust-lang/crates.io-index"),
            "automatic discovery supports crates.io dependencies only; use --candidate-path for local experiments"
        );

        if !args.versions.is_empty() {
            for version in &args.versions {
                ensure!(
                    dependency.req.matches(version),
                    "candidate {version} is outside {}",
                    dependency.req
                );
                candidates.push(Candidate {
                    version: version.clone(),
                    path: None,
                    rust_version: None,
                });
            }
        } else {
            ensure!(
                !args.offline,
                "automatic version discovery needs network access; use --versions or --candidate-path with --offline"
            );
            let name = dependency.name.to_ascii_lowercase();
            ensure!(
                !name.is_empty()
                    && name.is_ascii()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
                "invalid registry package name"
            );
            let index_path = match name.len() {
                1 => format!("1/{name}"),
                2 => format!("2/{name}"),
                3 => format!("3/{}/{name}", &name[..1]),
                _ => format!("{}/{}/{name}", &name[..2], &name[2..4]),
            };
            let body = ureq::get(format!("https://index.crates.io/{index_path}"))
                .call()
                .context("fetch crates.io release list")?
                .body_mut()
                .read_to_string()
                .context("read crates.io release list")?;

            for line in body.lines() {
                let release: Release = serde_json::from_str(line)?;

                if !release.yanked && dependency.req.matches(&release.vers) {
                    candidates.push(Candidate {
                        version: release.vers,
                        path: None,
                        rust_version: release
                            .rust_version
                            .as_deref()
                            .map(rust_version)
                            .transpose()?,
                    });
                }
            }
        }
    }

    candidates.sort_by(|left, right| left.version.cmp(&right.version));
    candidates.dedup_by(|left, right| left.version == right.version);
    Ok((candidates, exhaustive))
}
