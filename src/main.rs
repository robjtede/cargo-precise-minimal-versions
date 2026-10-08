mod candidates;
mod cargo;
mod probe;

use std::{fs, path::PathBuf};

use anyhow::{Context, Result, ensure};
use clap::Parser;
use semver::Version;
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Probe direct dependency API requirements independently of Cargo's version unification"
)]
struct Args {
    #[arg(long, default_value = "Cargo.toml")]
    manifest_path: PathBuf,

    /// Dependency key in the selected library's manifest.
    #[arg(long)]
    dependency: String,

    #[arg(short, long)]
    package: Option<String>,

    /// Nightly Cargo toolchain used for temporary lockfiles.
    #[arg(long, default_value = "nightly")]
    toolchain: String,

    /// Compiler toolchain; defaults to the package MSRV, or --toolchain.
    #[arg(long)]
    rustc_toolchain: Option<String>,

    #[arg(long, value_delimiter = ',')]
    features: Vec<String>,

    #[arg(long)]
    no_default_features: bool,

    /// Probe only these versions. The report will mark the search as sampled.
    #[arg(long, value_delimiter = ',')]
    versions: Vec<Version>,

    /// Local candidate crate directory. Repeat for offline experiments.
    #[arg(long, conflicts_with = "versions")]
    candidate_path: Vec<PathBuf>,

    /// Directory for a new report; must not already exist.
    #[arg(long)]
    report_dir: Option<PathBuf>,

    #[arg(long)]
    offline: bool,

    #[arg(long)]
    json: bool,
}

#[derive(Serialize)]
struct Report {
    package: String,
    dependency: String,
    declared_requirement: String,
    resolved_version: Version,
    compiler: String,
    target: String,
    features: Vec<String>,
    default_features: bool,
    candidate_features: Vec<String>,
    exhaustive: bool,
    minimum_verified: bool,
    lowest_passing: Option<Version>,
    recommendation: Option<String>,
    probes: Vec<probe::Probe>,
    limitations: Vec<&'static str>,
}

fn main() {
    let result = if std::env::var_os(probe::CAPTURE_ENV).is_some() {
        probe::wrapper()
    } else {
        run().map(|()| 0)
    };

    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("error: {error:#}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<()> {
    let mut arguments = std::env::args_os().collect::<Vec<_>>();

    if arguments
        .get(1)
        .is_some_and(|arg| arg == "precise-minimal-versions")
    {
        arguments.remove(1);
    }

    let mut args = Args::parse_from(arguments);
    args.manifest_path = args
        .manifest_path
        .canonicalize()
        .context("read manifest path")?;

    let work = tempfile::tempdir().context("create probe directory")?;
    let mut cargo = cargo::Cargo::new(&args, work.path())?;
    let package = cargo.package(&args)?;
    cargo.select_compiler(&args, &package)?;
    let dependency = package.dependency(&args.dependency)?;
    let candidate_features = package.dependency_features(&args, dependency)?;
    let (candidates, exhaustive) = candidates::list(&args, dependency)?;
    ensure!(
        !candidates.is_empty(),
        "no candidate versions match the declared requirement"
    );

    let report_dir = match &args.report_dir {
        Some(path) => path.clone(),
        None => {
            let parent = args
                .manifest_path
                .parent()
                .context("manifest has no parent")?;
            let reports = parent.join("target/precise-minimal-versions");
            fs::create_dir_all(&reports)?;
            tempfile::Builder::new()
                .prefix("run-")
                .tempdir_in(reports)?
                .keep()
        }
    };

    if args.report_dir.is_some() {
        fs::create_dir(&report_dir).context("create report directory (it must not exist)")?;
    }

    let report_dir = report_dir.canonicalize()?;
    eprintln!("Building baseline for {}…", package.name);
    let baseline = cargo.baseline(&args, &package, dependency, &report_dir)?;
    let mut report = Report {
        package: package.name.clone(),
        dependency: args.dependency.clone(),
        declared_requirement: dependency.req.to_string(),
        resolved_version: baseline.resolved_version.clone(),
        compiler: cargo.compiler_version.clone(),
        target: cargo.host.clone(),
        features: args.features.clone(),
        default_features: !args.no_default_features,
        candidate_features: candidate_features.clone(),
        exhaustive,
        minimum_verified: false,
        lowest_passing: None,
        recommendation: None,
        probes: Vec::new(),
        limitations: vec![
            "Compile evidence only; runtime behavior is not tested.",
            "Other packages retain baseline versions and features. Shared dependency types can make a probe inconclusive.",
            "Build scripts and procedural macros retain their baseline configuration.",
            "Only the selected host library configuration is tested; each dependency is probed separately.",
        ],
    };
    let mut conclusive = true;

    for candidate in candidates {
        eprintln!("Probing {} {}…", args.dependency, candidate.version);
        let result = cargo.probe(
            &args,
            dependency,
            &candidate_features,
            &candidate,
            &baseline,
            &report_dir,
        )?;
        let passed = result.outcome == probe::Outcome::Passed;

        if !matches!(
            result.outcome,
            probe::Outcome::Passed | probe::Outcome::ApiFailure
        ) {
            conclusive = false;
        }

        if passed {
            report.lowest_passing = Some(candidate.version.clone());

            if conclusive && exhaustive {
                report.minimum_verified = true;
                let floor = &dependency.req.comparators[0];
                let mut declared_floor = Version::new(
                    floor.major,
                    floor.minor.unwrap_or(0),
                    floor.patch.unwrap_or(0),
                );
                declared_floor.pre = floor.pre.clone();

                if candidate.version > declared_floor
                    && report
                        .probes
                        .iter()
                        .any(|probe| probe.outcome == probe::Outcome::ApiFailure)
                {
                    report.recommendation = Some(candidate.version.to_string());
                }
            }
        }

        report.probes.push(result);
        fs::write(
            report_dir.join("report.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;

        if passed {
            break;
        }
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "{}: declared {}, graph selected {}",
            report.dependency, report.declared_requirement, report.resolved_version
        );

        for result in &report.probes {
            println!(
                "  {}: {}",
                result.version,
                serde_json::to_value(result.outcome)?
            );
        }

        match (&report.recommendation, &report.lowest_passing) {
            (Some(version), _) => println!(
                "Suggested direct requirement: {} = {version:?}",
                report.dependency
            ),
            (_, Some(_)) if report.minimum_verified => {
                println!("First eligible version passed; no change needed")
            }
            (_, Some(version)) => {
                println!("Lowest passing probe: {version}; minimum remains unverified")
            }
            _ => println!("Direct requirement remains unknown"),
        }

        println!("Report: {}", report_dir.join("report.json").display());
    }

    Ok(())
}
