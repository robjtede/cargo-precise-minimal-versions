use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, ensure};
use semver::Version;
use serde::{Deserialize, Serialize};

pub const CAPTURE_ENV: &str = "CPMV_CAPTURE";

#[derive(Deserialize, Serialize)]
pub struct Invocation {
    compiler: PathBuf,
    args: Vec<String>,
    directory: PathBuf,
    environment: BTreeMap<String, String>,
}

#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Passed,
    ApiFailure,
    Inconclusive,
    CandidateBuildFailed,
    RequiresNewerRust,
}

#[derive(Serialize)]
pub struct Probe {
    pub version: Version,
    pub outcome: Outcome,
    pub diagnostics: PathBuf,
}

pub fn wrapper() -> Result<i32> {
    let mut arguments = env::args().skip(1);
    let compiler = arguments.next().context("missing wrapped compiler")?;
    let args = arguments.collect::<Vec<_>>();
    let root = env::var("CPMV_ROOT")?;
    let crate_name = env::var("CPMV_CRATE")?;
    let is_root = env::var("CARGO_MANIFEST_DIR").is_ok_and(|value| {
        Path::new(&value).canonicalize().ok() == Path::new(&root).canonicalize().ok()
    }) && args
        .windows(2)
        .any(|pair| pair[0] == "--crate-name" && pair[1] == crate_name);

    if is_root {
        let environment = env::vars()
            .filter(|(key, _)| {
                key.starts_with("CARGO_PKG_")
                    || key.starts_with("DEP_")
                    || matches!(
                        key.as_str(),
                        "CARGO_MANIFEST_DIR"
                            | "CARGO_MANIFEST_PATH"
                            | "CARGO_CRATE_NAME"
                            | "CARGO_PRIMARY_PACKAGE"
                            | "CARGO"
                            | "PATH"
                            | "LD_LIBRARY_PATH"
                            | "DYLD_FALLBACK_LIBRARY_PATH"
                            | "OUT_DIR"
                    )
            })
            .collect();
        let invocation = Invocation {
            compiler: PathBuf::from(&compiler),
            args: args.clone(),
            directory: env::current_dir()?,
            environment,
        };

        fs::write(env::var(CAPTURE_ENV)?, serde_json::to_vec(&invocation)?)?;
    }

    Ok(Command::new(compiler)
        .args(args)
        .status()?
        .code()
        .unwrap_or(1))
}

impl Invocation {
    pub fn build_environment(&mut self, output: &[u8], package_id: &str) -> Result<()> {
        for line in String::from_utf8_lossy(output).lines() {
            let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };

            if message["reason"] == "build-script-executed" && message["package_id"] == package_id {
                let entries: Vec<(String, String)> =
                    serde_json::from_value(message["env"].clone())?;
                self.environment.extend(entries);
            }
        }

        Ok(())
    }

    pub fn dependency(&self, name: &str) -> Result<PathBuf> {
        self.args
            .windows(2)
            .filter(|pair| pair[0] == "--extern")
            .find_map(|pair| pair[1].strip_prefix(&format!("{name}=")).map(PathBuf::from))
            .context("candidate compiler invocation has no direct dependency artifact")
    }

    pub fn search_paths(&self) -> Vec<String> {
        self.args
            .windows(2)
            .filter(|pair| pair[0] == "-L" && pair[1].starts_with("dependency="))
            .map(|pair| pair[1].clone())
            .collect()
    }

    pub fn read(path: &Path) -> Result<Self> {
        serde_json::from_slice(
            &fs::read(path).context("baseline did not capture the library compiler invocation")?,
        )
        .context("read captured compiler invocation")
    }

    pub fn run(
        &self,
        dependency: &str,
        artifact: &Path,
        search_paths: &[String],
        output_dir: &Path,
    ) -> Result<std::process::Output> {
        fs::create_dir_all(output_dir)?;
        let dependency = dependency.replace('-', "_");
        let mut args = Vec::new();
        let mut replaced = 0;
        let mut input = self.args.iter();

        while let Some(arg) = input.next() {
            match arg.as_str() {
                "--extern" => {
                    let value = input.next().context("missing --extern value")?;
                    let name = value.split('=').next().unwrap_or_default();
                    args.push(arg.clone());

                    if name == dependency {
                        args.push(format!("{dependency}={}", artifact.display()));
                        replaced += 1;
                    } else {
                        args.push(value.clone());
                    }
                }
                "--out-dir" | "--emit" => {
                    input.next().context("missing compiler argument")?;
                    args.push(arg.clone());
                    args.push(if arg == "--out-dir" {
                        output_dir.display().to_string()
                    } else {
                        "metadata".into()
                    });
                }
                "-C" => {
                    let value = input.next().context("missing -C value")?;

                    if !value.starts_with("incremental=") {
                        args.push(arg.clone());
                        args.push(value.clone());
                    }
                }
                _ if arg.starts_with("--emit=") => args.push("--emit=metadata".into()),
                _ => args.push(arg.clone()),
            }
        }

        ensure!(
            replaced == 1,
            "expected one active direct --extern for {dependency}, found {replaced}"
        );
        for path in search_paths {
            args.extend(["-L".into(), path.clone()]);
        }

        Command::new(&self.compiler)
            .args(args)
            .envs(&self.environment)
            .current_dir(&self.directory)
            .output()
            .context("run direct-use compiler probe")
    }
}

pub fn classify(output: &std::process::Output) -> Outcome {
    if output.status.success() {
        return Outcome::Passed;
    }

    let errors = String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| message["level"] == "error")
        .collect::<Vec<_>>();
    let coded_errors = errors
        .iter()
        .filter_map(|error| error["code"]["code"].as_str())
        .collect::<Vec<_>>();

    let ambiguous = errors.iter().any(|error| {
        let text = error.to_string();
        text.contains("trait bounds were not satisfied")
            || text.contains("multiple different versions")
            || text.contains("different versions of crate")
            || error["spans"]
                .as_array()
                .is_some_and(|spans| spans.iter().any(|span| !span["expansion"].is_null()))
    });

    // Type identity and infrastructure errors cannot establish an API introduction.
    if !ambiguous
        && !coded_errors.is_empty()
        && coded_errors.iter().all(|code| {
            matches!(
                *code,
                "E0425" | "E0432" | "E0433" | "E0405" | "E0407" | "E0412" | "E0599"
            )
        })
        && errors.iter().all(|error| {
            !error["code"].is_null()
                || error["message"]
                    .as_str()
                    .is_some_and(|message| message.starts_with("aborting due to"))
        })
    {
        Outcome::ApiFailure
    } else {
        Outcome::Inconclusive
    }
}
