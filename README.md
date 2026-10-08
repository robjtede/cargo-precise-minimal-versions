# cargo-precise-minimal-versions

Find the dependency versions required by a library's own code, independently of
higher requirements imposed by other packages.

Experimental prototype. It reports suggestions and does not edit your manifests.
It only searches within the declared requirement; it does not lower an existing
bound or cross a compatibility boundary.

## Usage

Install a recent nightly toolchain. The tool currently requires nightly Cargo
1.97 or newer for its temporary lockfile support.

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

The compilation toolchain defaults to the selected package's `rust-version`,
including an inherited workspace value. If no MSRV is declared, it uses nightly.
The compiler toolchain must already be installed. Override it explicitly with:

```sh
cargo precise-minimal-versions --dependency serde --rustc-toolchain 1.85.0
```

Use `--features a,b` and `--no-default-features` to select the library configuration.
Only the compiler's host target is tested. Nightly Cargo performs dependency
resolution; the selected compiler builds both the baseline and the candidates.

For a short experiment, specify a sample of versions:

```sh
cargo precise-minimal-versions \
  --dependency serde \
  --versions 1.0.0,1.0.59,1.0.60
```

A sampled search reports the lowest passing probe, but does not issue a minimum
version recommendation. `--candidate-path /path/to/crate` can be repeated for
local experiments. These are also treated as samples. `--offline` works with
explicit versions that are already cached, or with local candidates.

## How it works

Suppose your library needs `widget` 1.1, while another dependency needs 1.2.
The appropriate direct requirement is still `widget = "1.1"`. A Cargo downgrade
alone cannot test that requirement if the graph rejects 1.1.

The tool bypasses version unification at the compiler boundary:

1. Build a passing baseline and capture the library's rustc invocation.
2. Read non-yanked releases from the crates.io index in ascending version order.
3. Build each candidate in a separate temporary project, using only dependency
   features requested by the selected library.
4. Replay the library's compiler invocation with its direct `--extern` argument
   replaced by the candidate artifact. Other packages retain their baseline
   dependencies. Reuse the candidate's dependency search paths and preserve the
   library's build-script environment and generated files.
5. Stop at the first passing probe. Suggest an increased caret requirement only
   if at least one earlier eligible release failed and every such failure was a
   recognized missing-API error. If the first eligible version passes, suggest no
   change. Yanked releases are skipped silently; gaps in published or eligible
   versions do not justify raising the declared bound.

Exact requirements occur only in disposable candidate projects. The user's
manifest and lockfile stay unchanged. Suggested requirements use ordinary caret
syntax, such as `serde = "1.0.60"`.

The baseline uses a separate lockfile, so an existing project lockfile does not
constrain the search. Build artifacts are temporary. Reports remain under
`target/precise-minimal-versions/run-*` next to the supplied manifest. Set
`--report-dir PATH` to use a different, new directory.

## Evidence and limits

`--json` emits the report on standard output. Progress goes to standard error.
The report includes the declared requirement, graph-selected version, compiler,
target, features, tested versions, lowest passing version, and recommendation.
It is also saved as `report.json`, with compiler diagnostics and resolved graphs.
The captured baseline invocation is retained as evidence; its temporary artifact
paths do not remain usable after the run finishes.

`exhaustive` describes candidate discovery. `minimum_verified` is true only when
all earlier eligible candidates failed with recognized missing-API diagnostics
and a later candidate passed. This verifies the minimum **within this compiler
probe model**, not every possible dependency graph or runtime behavior.

The tool distinguishes:

- `api_failure`: recognized missing function, type, import, or method diagnostics.
- `passed`: the selected library compiled against the candidate.
- `inconclusive`: shared-type conflicts, macro-related failures, or other errors
  that do not establish a direct API requirement.
- `candidate_build_failed`: the isolated candidate could not resolve or compile.
- `requires_newer_rust`: the candidate declares a newer MSRV than the compiler.

An inconclusive earlier candidate prevents a recommendation. A higher requirement
from another package never becomes evidence for raising the direct bound.
The command exits successfully when analysis completes, including inconclusive
reports. Operational failures exit with status 1.

Important prototype limits:

- Cross-crate type identities can make substitution fail even when the old API
  is sufficient. Such results remain inconclusive. The prototype cannot yet
  solve the direct floor for all libraries that exchange dependency types.
- Build scripts, generated source, and procedural macros retain their baseline
  configuration. The probe does not re-evaluate version-dependent code generation.
- Missing Cargo features and failures inside old dependencies remain unresolved;
  they do not trigger a higher recommendation. Missing MSRV metadata requires an
  actual build. Release dates do not establish a lower version bound.
- Only normal Rust library dependencies with a single caret requirement are
  supported. Renamed dependencies and simple feature forwarding are supported.
  Target-specific dependency declarations, build-dependency searches, proc-macro
  candidates, automatic searches of patched sources, and non-crates.io registries
  are not supported.
- It checks one dependency and one feature configuration at a time. It does not
  test runtime behavior, all features, all targets, or simultaneous minimums.
- Existing compiler wrappers are disabled during probes. Build scripts execute
  as they do in a normal Cargo build.

## Serde example

The included example implements `Serialize` for an `i128` wrapper and declares
`serde = "1"`. A second dependency requires Serde 1.0.100 or newer.

```sh
cargo +nightly run --locked -- \
  --manifest-path examples/serde-type/Cargo.toml \
  --dependency serde
```

A verified run on the macOS ARM64 host with rustc 1.101.0-nightly selected Serde
1.0.229 for the baseline. It probed 60 non-yanked releases through 1.0.60: the
59 earlier candidates failed, and 1.0.60 passed. The result was:

```text
Suggested direct requirement: serde = "1.0.60"
```

These results describe that example and configuration, not a universal Serde
requirement.

## Development

```sh
just fmt-check
just test --offline
just clippy
```

The integration tests use local packages and need no registry access after the
tool's own dependencies are cached. They cover a higher transitive requirement,
shared dependency types, feature isolation and forwarding, build-script
variables, MSRV filtering, and manifest/lockfile preservation.

## References

- [Cargo compiler wrappers](https://doc.rust-lang.org/cargo/reference/environment-variables.html)
- [rustc external dependencies](https://doc.rust-lang.org/rustc/command-line-arguments.html#--extern-specify-where-an-external-library-is-located)
- [Cargo dependency resolution](https://doc.rust-lang.org/cargo/reference/resolver.html)
- [cargo-minimal-versions](https://github.com/taiki-e/cargo-minimal-versions)
