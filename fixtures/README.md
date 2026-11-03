# Diagnostic fixtures

Each directory is an independent library project. Its dependencies and candidate versions are local path packages. The root project compiles normally; each case fails when the tool isolates a candidate version or its features.

Run a fixture from the repository root:

```sh
just fixture api-failure
just fixture missing-feature
just fixture shared-types
just fixture candidate-build-failed
just fixture requires-newer-rust
```

The recipe uses `--offline`, `--search linear`, and two `--candidate-path` arguments. Build the tool's dependencies once before working offline. Nightly Cargo 1.97 or newer must be installed. The fixtures use nightly rustc by default because they do not declare a package MSRV.

| Project                                                    | Failure mode                                                                                                               | Expected result                                                                              |
| ---------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| [api-failure](api-failure/README.md)                       | The library calls `widget::needed`, which is absent from 1.0.0.                                                            | 1.0.0: `api_failure`; 1.1.0: `passed`.                                                       |
| [missing-feature](missing-feature/README.md)               | `bridge` enables `widget/extra`, but the library does not request it.                                                      | Version probes: `inconclusive`. Feature check recommends `widget/extra`; its recheck passes. |
| [shared-types](shared-types/README.md)                     | `bridge` returns a type from the baseline Widget build. Substitution gives the direct dependency a distinct type identity. | 1.0.0: `inconclusive` with E0308; no version recommendation.                                 |
| [candidate-build-failed](candidate-build-failed/README.md) | Widget 1.0.0 contains an intentional `compile_error!`.                                                                     | 1.0.0: `candidate_build_failed`; 1.1.0: `passed`.                                            |
| [requires-newer-rust](requires-newer-rust/README.md)       | Widget 1.0.0 declares the synthetic requirement `rust-version = "99.0"`.                                                   | 1.0.0: `requires_newer_rust`; 1.1.0: `passed`.                                               |

Each project uses Widget 1.1.0 from `baseline/`. The alternate 1.0.0 package is in `candidate/`. The feature and type cases also have a `bridge/` package that preserves the baseline dependency graph.

These are sampled searches. Every report has `exhaustive: false`, `minimum_verified: false`, and no version recommendation. A passing 1.1.0 probe does not verify the minimum version.

Each project also has a `Cargo.toml.suggested` that shows the manifest after correction:

- `api-failure` requires Widget 1.1.0, which provides the missing API.
- `missing-feature` requests `widget/extra` directly.
- `candidate-build-failed` requires Widget 1.1.0 to exclude the intentionally broken candidate.
- `requires-newer-rust` requires Widget 1.1.0 to exclude the synthetic Rust 99.0 candidate.
- `shared-types` keeps the original declaration and explains the probe limitation. A normal Cargo build already preserves the shared type identity; no manifest correction is required.

The version changes above are manual examples based on the known local fixture sources. They are not verified minimum-version recommendations from the sampled reports. The missing-feature correction is verified by the tool's feature recheck.

To try a suggested manifest, copy the fixture to a temporary directory and replace its `Cargo.toml` with `Cargo.toml.suggested`. Keep the local dependency directories beside it. Probe only candidate versions that match the updated requirement; the version corrections exclude `candidate/` at 1.0.0, so use `--candidate-path` with `baseline/` at 1.1.0.

Append tool options to the recipe:

```sh
just fixture missing-feature --json
just fixture missing-feature --check
just fixture api-failure --rustc-toolchain 1.88.0
```

With `--check`, `missing-feature` exits with status 3 because the feature recommendation is verified. The other fixtures exit with status 0 because their sampled version probes do not establish a verified change. Candidate failures are analysis results, so they do not cause an operational failure exit.

Reports and compiler diagnostics are saved under each project's `target/precise-minimal-versions/run-*`. Use `--report-dir PATH` to select a new directory. The tool leaves fixture manifests and lockfiles unchanged.

The integration tests run all five projects with both `Cargo.toml` and `Cargo.toml.suggested`. Each test copies the project to a temporary directory, applies the selected manifest, and selects local candidates that match its requirement. The tests check the report and `--check` exit status, confirm that the saved JSON matches standard output, and verify that manifests and lockfiles stay unchanged.

The original manifests reproduce each failure mode. The suggested manifests verify that the API, feature, candidate-build, and compiler-requirement corrections pass. The shared-type case keeps its original requirement and remains inconclusive. All searches remain sampled and must not claim a verified minimum.

These ten tests are part of `just test` and run in CI. To run only the fixture tests:

```sh
just test --offline fixture_
```
