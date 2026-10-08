# cargo-precise-minimal-versions

Find the dependency versions required by a library's own code and detect dependency features missing from its manifest. Compiler probes isolate the library's needs from versions and features enabled by other packages.

Experimental prototype. It reports suggestions and does not edit your manifests. It only searches within the declared requirement; it does not lower an existing bound or cross a compatibility boundary.

## Usage

Install a recent nightly toolchain. The tool currently requires nightly Cargo 1.97 or newer for its temporary lockfile support.

```sh
cargo +nightly install --path . --locked

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
rustup toolchain install 1.88.0

nix develop
just check
just test --offline
just build-msrv
just --set toolchain 1.88.0 test --offline
```

The shell creates `.toolchain/rust.just` from the pinned x52 Nix input. With direnv, run `direnv allow` to enter the shell automatically. For a single command, use `nix develop -c just check`.

Use `just fmt` to format all project files, `just build` to build the binary, and `just run --help` to show command options. `just test` uses nextest and accepts additional arguments. Build, test, and Clippy commands use stable Rust by default; set the `toolchain` Just variable to select another installed compiler.

The default integration tests use local packages and need no registry access after the tool's own dependencies are cached. They cover a higher transitive requirement, shared dependency types, feature isolation and forwarding, verified feature suggestions in JSON and terminal output, build-script variables, MSRV filtering, and manifest/lockfile preservation.

CI tests the binary with stable Rust and installs nightly Cargo for compiler probes. Separate jobs check formatting, Clippy, unused dependencies, workflow syntax, and GitHub Actions security.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.

## References

- [Cargo compiler wrappers](https://doc.rust-lang.org/cargo/reference/environment-variables.html)
- [rustc external dependencies](https://doc.rust-lang.org/rustc/command-line-arguments.html#--extern-specify-where-an-external-library-is-located)
- [Cargo dependency resolution](https://doc.rust-lang.org/cargo/reference/resolver.html)
- [cargo-minimal-versions](https://github.com/taiki-e/cargo-minimal-versions)
