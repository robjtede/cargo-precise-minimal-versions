use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::probe::Probe;

#[derive(Serialize)]
pub struct FeatureCheck {
    pub probe: Probe,
    pub missing_features: Vec<String>,
    pub recommended_features: Vec<String>,
    pub recheck_features: Vec<String>,
    pub recheck: Option<Probe>,
}

pub struct GatedError {
    pub features: BTreeSet<String>,
    diagnostic: Value,
}

pub fn errors(stderr: &[u8]) -> Vec<Value> {
    String::from_utf8_lossy(stderr)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| message["level"] == "error")
        .collect()
}

fn gated_feature(message: &str) -> Option<&str> {
    message
        .strip_prefix("the item is gated behind the `")?
        .strip_suffix("` feature")
}

pub fn has_feature_gate(diagnostic: &Value) -> bool {
    (diagnostic["level"] == "note"
        && (diagnostic["message"]
            .as_str()
            .and_then(gated_feature)
            .is_some()
            || diagnostic["spans"].as_array().is_some_and(|spans| {
                spans
                    .iter()
                    .any(|span| span["label"].as_str().and_then(gated_feature).is_some())
            })))
        || diagnostic["children"]
            .as_array()
            .is_some_and(|children| children.iter().any(has_feature_gate))
}

pub fn missing_features(
    stderr: &[u8],
    package: &Value,
    enabled: &[Value],
) -> Result<Vec<GatedError>> {
    let manifest = PathBuf::from(
        package["manifest_path"]
            .as_str()
            .context("candidate has no manifest path")?,
    );
    let root = manifest
        .parent()
        .context("candidate manifest has no parent")?
        .canonicalize()?;
    let features = package["features"]
        .as_object()
        .context("candidate has no feature definitions")?;
    let mut result = Vec::new();

    for diagnostic in errors(stderr) {
        let mut missing = BTreeSet::new();
        collect_features(&diagnostic, features, enabled, &root, &mut missing);

        if !missing.is_empty() {
            result.push(GatedError {
                features: missing,
                diagnostic,
            });
        }
    }

    Ok(result)
}

fn collect_features(
    diagnostic: &Value,
    available: &Map<String, Value>,
    enabled: &[Value],
    root: &Path,
    missing: &mut BTreeSet<String>,
) {
    if diagnostic["level"] == "note"
        && let Some(spans) = diagnostic["spans"].as_array()
    {
        for span in spans {
            let feature = span["label"]
                .as_str()
                .and_then(gated_feature)
                .or_else(|| diagnostic["message"].as_str().and_then(gated_feature));

            if let Some(feature) = feature
                && available.contains_key(feature)
                && !enabled.iter().any(|enabled| enabled == feature)
                && span["file_name"].as_str().is_some_and(|file| {
                    Path::new(file)
                        .canonicalize()
                        .is_ok_and(|path| path.starts_with(root))
                })
            {
                missing.insert(feature.to_owned());
            }
        }
    }

    if let Some(children) = diagnostic["children"].as_array() {
        for child in children {
            collect_features(child, available, enabled, root, missing);
        }
    }
}

pub fn verified_features(gates: &[GatedError], stderr: &[u8]) -> Vec<String> {
    let remaining = errors(stderr);
    let features = gates
        .iter()
        .flat_map(|gate| gate.features.iter().cloned())
        .collect::<BTreeSet<_>>();

    features
        .into_iter()
        .filter(|feature| {
            gates
                .iter()
                .filter(|gate| gate.features.contains(feature))
                .all(|gate| {
                    !remaining
                        .iter()
                        .any(|error| same_error(&gate.diagnostic, error))
                })
        })
        .collect()
}

fn same_error(before: &Value, after: &Value) -> bool {
    before["code"]["code"] == after["code"]["code"]
        && before["message"] == after["message"]
        && primary_locations(before) == primary_locations(after)
}

fn primary_locations(diagnostic: &Value) -> Vec<(&str, u64, u64)> {
    diagnostic["spans"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|span| span["is_primary"] == true)
        .filter_map(|span| {
            Some((
                span["file_name"].as_str()?,
                span["byte_start"].as_u64()?,
                span["byte_end"].as_u64()?,
            ))
        })
        .collect()
}
