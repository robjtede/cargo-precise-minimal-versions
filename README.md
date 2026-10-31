# cargo-precise-minimal-versions

Find the dependency versions required by a library's own code and detect dependency features missing from its manifest. Compiler probes isolate the library's needs from versions and features enabled by other packages.

Experimental prototype. It reports suggestions and does not edit your manifests. It only searches within the declared requirement; it does not lower an existing bound or cross a compatibility boundary.

## Installation

Install with [cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

```sh
cargo binstall cargo-precise-minimal-versions
```

Alternatively, build from source:

```sh
cargo --locked install cargo-precise-minimal-versions
```

## Usage

Install a recent nightly toolchain. The tool currently requires nightly Cargo 1.97 or newer for its temporary lockfile support.

```sh
cargo precise-minimal-versions --dependency serde
```

Select one normal dependency by its manifest key. For a workspace member:

```sh
cargo precise-minimal-versions \
  --manifest-path /path/to/workspace/Cargo.toml \
  --package my-library \
  --dependency serde
```

The compilation toolchain defaults to the selected package's `rust-version`, including an inherited workspace value. If no MSRV is declared, it uses nightly. The compiler toolchain must already be installed. Override it explicitly with:

```sh
cargo precise-minimal-versions --dependency serde --rustc-toolchain 1.85.0
```

Use `--features a,b` and `--no-default-features` to select the library configuration. Only the compiler's host target is tested. Nightly Cargo performs dependency resolution; the selected compiler builds both the baseline and the candidates.

By default, the search checks the oldest eligible release first. If it fails with a recognized missing-API error and more than ten releases remain, the search uses bisection. It then checks the remaining interval in ascending order. Use `--bisect-threshold N` to change the maximum interval size. The threshold counts published, eligible releases, not version numbers.

Bisection assumes that an API remains available in later releases. If earlier releases are skipped, the report gives the lowest passing probe with `minimum_verified: false` and no minimum recommendation. Use a linear search to check every earlier release:

```sh
cargo precise-minimal-versions --dependency serde --search linear
```

An inconclusive probe during bisection causes a linear scan. If bisection finds no passing release, the search also checks skipped releases before it finishes. Completed probes are reused.

To start with the newest eligible release and search downward, use:

```sh
cargo precise-minimal-versions --dependency serde_core --search descending
```

This checks every eligible published release in descending order and reports the lowest passing version. It continues after failures because an older release can still pass. An inconclusive result below the lowest passing version prevents minimum verification. The declared requirement still limits the search, and `--versions` still restricts it to the listed candidates.

For a short experiment, specify a sample of versions:

```sh
cargo precise-minimal-versions \
  --dependency serde \
  --versions 1.0.0,1.0.59,1.0.60
```

A sampled search reports the lowest passing probe, but does not issue a minimum version recommendation. `--candidate-path /path/to/crate` can be repeated for local experiments. These are also treated as samples. `--offline` works with explicit versions that are already cached, or with local candidates.

## Missing dependency features

Cargo combines dependency features across the graph. A library can compile because another package enables a feature that its own manifest omits. The tool checks for this on every run, including sampled searches.

For example, a library that uses `tokio::time` must request Tokio's `time` feature. If its direct declaration requests only `sync` and `macros`, another dependency can hide the omission:

```toml
tokio = { version = "1.38.2", features = ["sync", "macros"] }
```

Run the same command used for version checks:

```sh
cargo precise-minimal-versions --dependency tokio
```

The tool probes the graph-selected version with only the features requested by your library. When rustc identifies a feature gate in the selected dependency, the tool adds that feature in a temporary project and repeats the probe. It recommends the feature only if the original missing-API errors disappear:

```text
Suggested direct feature: tokio/time
```

Add the feature to the existing dependency declaration:

```toml
tokio = { version = "1.38.2", features = ["sync", "macros", "time"] }
```

The JSON report includes `feature_check`: the original probe, `missing_features`, `recommended_features`, `recheck_features`, and the recheck result. Diagnostics and resolved graphs for both probes are saved with the report.

A feature recommendation verifies that the named feature removes the reported gate errors at the graph-selected version. Other compilation errors can remain, including shared-type conflicts. It does not verify the complete build or the minimum version. The version search keeps the original feature configuration, so a missing feature does not justify raising a version bound.

This check uses explicit rustc feature-gate diagnostics. It does not infer every required feature, attribute a transitive crate's gate to the selected dependency, or search for a minimal feature set. A failed recheck leaves the detected feature unverified.

## How it works

Suppose your library needs `widget` 1.1, while another dependency needs 1.2. The appropriate direct requirement is still `widget = "1.1"`. A Cargo downgrade alone cannot test that requirement if the graph rejects 1.1.

The tool bypasses version unification at the compiler boundary:

1. Build a passing baseline and capture the library's rustc invocation.
2. Read non-yanked releases from the crates.io index in ascending version order.
3. Select candidates with the configured search strategy. Build each selected candidate in a separate temporary project, using only dependency features requested by the selected library.
4. Replay the library's compiler invocation with its direct `--extern` argument replaced by the candidate artifact. Other packages retain their baseline dependencies. Reuse the candidate's dependency search paths and preserve the library's build-script environment and generated files.
5. Finish the linear scan or the final bisection interval at its first passing release. Suggest an increased caret requirement only if at least one earlier eligible release failed and every earlier eligible release was checked and failed with a recognized missing-API error. If the first eligible version passes, suggest no change. Yanked releases are skipped silently; gaps in published or eligible versions do not justify raising the declared bound.

Exact requirements occur only in disposable candidate projects. The user's manifest and lockfile stay unchanged. Suggested requirements use ordinary caret syntax, such as `serde = "1.0.60"`.

The baseline uses a separate lockfile, so an existing project lockfile does not constrain the search. Build artifacts are temporary. Reports remain under `target/precise-minimal-versions/run-*` next to the supplied manifest. Set `--report-dir PATH` to use a different, new directory.

## Evidence and limits

`--json` emits the report on standard output. Progress goes to standard error. The report includes the declared requirement, graph-selected version, compiler, target, features, feature-check evidence, search strategy, bisection threshold, tested versions, lowest passing version, and recommendation. Probes appear in execution order. It is also saved as `report.json`, with compiler diagnostics and resolved graphs. The captured baseline invocation is retained as evidence; its temporary artifact paths do not remain usable after the run finishes.

`exhaustive` describes candidate discovery. It can remain true when bisection skips probes. `minimum_verified` is true only when discovery was exhaustive, all earlier eligible candidates were checked and failed with recognized missing-API diagnostics, and a candidate passed. This verifies the minimum **within this compiler probe model**, not every possible dependency graph or runtime behavior.

The tool distinguishes:

- `api_failure`: recognized missing function, type, import, or method diagnostics.
- `passed`: the selected library compiled against the candidate.
- `inconclusive`: missing features, shared-type conflicts, macro-related failures, or other errors that do not establish a direct API requirement.
- `candidate_build_failed`: the isolated candidate could not resolve or compile.
- `requires_newer_rust`: the candidate declares a newer MSRV than the compiler.

An inconclusive earlier candidate prevents a recommendation. A higher requirement from another package never becomes evidence for raising the direct bound. The command exits successfully when analysis completes, including inconclusive reports. Operational failures exit with status 1.

With `--check`, a completed analysis exits with status 3 when it recommends a verified version increase or a verified missing feature. The report is saved and printed before the command exits. Without `--check`, findings do not change the exit status.

## Use in CI

Select one library, one dependency, and one feature configuration per command. Use `--check --search linear` to fail on verified changes and check all eligible releases before a passing candidate. Inconclusive probes, sampled searches, and bisection results do not justify a version increase. A verified missing feature can still fail the check when the minimum version is inconclusive.

Exit codes:

- `0`: analysis completed; with `--check`, no verified change was found. This does not imply that the minimum was verified. Inspect `minimum_verified` and `feature_check` in the JSON report.
- `1`: an operational failure, such as a failed baseline build, unavailable compiler, or registry error.
- `2`: invalid command-line arguments.
- `3`: `--check` found a verified version or feature change.

Pin the tool release and nightly toolchain. Install the target crate's MSRV separately; the binary's build compiler does not select the probe compiler. For workspace features, use package-qualified names such as `httpea/http-1_1`.

This GitHub Actions job is ready to adapt after version `0.1.0` is published. Change the package, dependency, features, and MSRV for your repository. Add matrix entries or jobs for additional checks.

```yaml
name: Dependency requirements

on:
  pull_request:
  workflow_dispatch:

permissions:
  contents: read

jobs:
  dependencies:
    runs-on: ubuntu-latest
    timeout-minutes: 30
    env:
      CPMV_VERSION: '0.1.0'
      NIGHTLY_TOOLCHAIN: nightly-2026-08-05
      PROBE_TOOLCHAIN: '1.92.0'
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false

      - name: Install compilers and dependency checker
        run: |
          rustup toolchain install stable "$NIGHTLY_TOOLCHAIN" "$PROBE_TOOLCHAIN" --profile minimal
          cargo +stable install cargo-precise-minimal-versions --version "$CPMV_VERSION" --locked

      - name: Check direct dependency requirements
        run: |
          cargo precise-minimal-versions \
            --manifest-path Cargo.toml \
            --package httpea \
            --dependency winnow \
            --features httpea/http-1_1 \
            --toolchain "$NIGHTLY_TOOLCHAIN" \
            --rustc-toolchain "$PROBE_TOOLCHAIN" \
            --search linear \
            --check \
            --report-dir target/dependency-reports/httpea-winnow

      - name: Upload reports and compiler diagnostics
        if: ${{ !cancelled() }}
        uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4.6.2
        with:
          name: dependency-reports
          path: target/dependency-reports/
          if-no-files-found: warn
```

Before the first release, install from a reviewed commit containing `--check` instead:

```sh
cargo +stable install \
  --git https://github.com/robjtede/cargo-precise-minimal-versions \
  --rev "$CPMV_REVISION" --locked cargo-precise-minimal-versions
```

Set `CPMV_REVISION` to the full commit SHA. Each explicit report directory must be new; missing parent directories are created. Keep report directories unique across matrix entries and reruns. Reports can contain partial compiler diagnostics after an operational failure, even when `report.json` could not be produced.

The tool reports findings; it does not edit the checked project's manifests or lockfile. It does not scan a workspace automatically, probe development or build dependencies, or prove runtime compatibility. An application using shared dependency types may need a separate locked Cargo build to resolve inconclusive results.

Important prototype limits:

- Cross-crate type identities can make substitution fail even when the old API is sufficient. Such results remain inconclusive. The prototype cannot yet solve the direct floor for all libraries that exchange dependency types.
- Build scripts, generated source, and procedural macros retain their baseline configuration. The probe does not re-evaluate version-dependent code generation.
- Feature recommendations are checked at the graph-selected version. Feature gates in older candidates and failures inside old dependencies do not justify a higher version requirement. Missing MSRV metadata requires an actual build. Release dates do not establish a lower version bound.
- Only normal Rust library dependencies with a single caret requirement are supported. Renamed dependencies and simple feature forwarding are supported. Target-specific dependency declarations, build-dependency searches, proc-macro candidates, automatic searches of patched sources, and non-crates.io registries are not supported.
- It checks one dependency and one feature configuration at a time. It does not test runtime behavior, all features, all targets, or simultaneous minimums.
- Existing compiler wrappers are disabled during probes. Build scripts execute as they do in a normal Cargo build.

## Serde example

The included example implements `Serialize` for an `i128` wrapper and declares `serde = "1"`. A second dependency requires Serde 1.0.100 or newer.

```sh
cargo +nightly run --locked -- \
  --manifest-path examples/serde-type/Cargo.toml \
  --dependency serde \
  --search linear
```

A verified run on the macOS ARM64 host with rustc 1.101.0-nightly selected Serde 1.0.229 for the baseline. It probed 60 non-yanked releases through 1.0.60: the 59 earlier candidates failed, and 1.0.60 passed. The result was:

```text
Suggested direct requirement: serde = "1.0.60"
```

The default bisection search found the same passing release with eight probes on the same compiler and host. It skipped earlier eligible releases, so it reported `minimum_verified: false` and no recommendation.

These results describe that example and configuration, not a universal Serde requirement.

## Real-world regression

The pinned `roxmltree` regression uses an unchanged upstream library that declares `memchr = "2"` but calls `memchr_iter`. With default features disabled, Memchr 2.0.0, 2.0.1, and 2.0.2 fail with a missing-import error; 2.1.0 passes. The exhaustive search verifies that API minimum and recommends `memchr = "2.1.0"` for this configuration.

Run the regression with:

```sh
just test-real-world
```

It needs GitHub and crates.io access, nightly Cargo, and Rust 1.88.0. It clones the recorded upstream revision into a temporary directory. The normal offline test suite skips it. See the [case record](docs/real-world-case.md) for sources, reproduction commands, and the separate default-feature requirement.

## Development

Rust 1.88 or newer builds the binary. Compiler probes also need nightly Cargo 1.97 or newer, including when the binary is built with stable Rust or the MSRV. Rust toolchains are managed with rustup; the Nix shell supplies development tools.

```sh
rustup toolchain install stable --component clippy
rustup toolchain install nightly --component rustfmt
rustup toolchain install 1.76.0
rustup toolchain install 1.88.0

nix develop
just check
just test --offline
just build-msrv
just --set toolchain 1.88.0 test --offline
```

The shell creates `.toolchain/rust.just` from the pinned x52 Nix input. With direnv, run `direnv allow` to enter the shell automatically. For a single command, use `nix develop -c just check`.

Use `just fmt` to format all project files, `just build` to build the binary, and `just run --help` to show command options. `just test` uses nextest and accepts additional arguments. Build, test, and Clippy commands use stable Rust by default; set the `toolchain` Just variable to select another installed compiler.

The default integration tests use local packages and need no registry access after the tool's own dependencies are cached. They need Rust 1.76.0 for the older-compiler regression. They cover a higher transitive requirement, shared dependency types, feature isolation and forwarding, verified feature suggestions in JSON and terminal output, build-script variables, MSRV filtering, and manifest/lockfile preservation.

CI tests the binary with stable Rust and installs nightly Cargo and Rust 1.76.0 for compiler probes. Separate jobs check formatting, Clippy, unused dependencies, workflow syntax, and GitHub Actions security.

## Releases

The release-plz workflow follows the binary release setup used by `dependabot-reviewer`: draft release PRs, crates.io publishing, draft GitHub releases, binary archives for macOS/Linux/Windows, SHA-256 checksums, and artifact attestations. The shared x52 release helpers comment on the release PR after publishing and after all assets are uploaded. Publish the draft GitHub release when the assets are ready.

Before enabling releases:

1. Publish the crate's first version manually. New crates require an initial authenticated publication before trusted publishing can be configured.
2. Configure a crates.io trusted publisher for `robjtede/cargo-precise-minimal-versions`, workflow `release.yml`, with no environment restriction.
3. Allow GitHub Actions to create pull requests in the repository settings.

The release workflow obtains a short-lived crates.io token through OIDC. It does not require a stored registry token. Release-plz generates the changelog and version updates from commits. The `ci-release` Nix shell supplies the same x52 release helpers as the other binary repositories.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.

## References

- [Cargo compiler wrappers](https://doc.rust-lang.org/cargo/reference/environment-variables.html)
- [rustc external dependencies](https://doc.rust-lang.org/rustc/command-line-arguments.html#--extern-specify-where-an-external-library-is-located)
- [Cargo dependency resolution](https://doc.rust-lang.org/cargo/reference/resolver.html)
- [cargo-minimal-versions](https://github.com/taiki-e/cargo-minimal-versions)
