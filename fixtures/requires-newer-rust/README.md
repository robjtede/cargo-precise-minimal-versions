# Candidate requires a newer Rust compiler

Run from the repository root:

```sh
just fixture requires-newer-rust
```

Expected output includes:

```text
  widget 1.0.0: requires a newer Rust compiler
  widget 1.1.0: passed
```

Widget 1.0.0 declares the synthetic requirement `rust-version = "99.0"`. The tool detects that this exceeds the selected compiler version and skips the candidate build. This is a compiler requirement, not evidence of a missing dependency API.

Use a release that supports the selected compiler. Widget 1.1.0 builds with the fixture's compiler. [Cargo.toml.suggested](Cargo.toml.suggested) excludes the synthetic Rust 99.0 candidate:

```toml
widget = { version = "1.1.0", path = "baseline" }
```

The suggested-manifest test probes 1.1.0 and passes. For a real dependency with an available newer compiler, another option is to install that compiler and select it with `--rustc-toolchain`.

The version change here is a manual correction for the known fixture sources. The original sampled report makes no minimum-version recommendation because the skipped candidate was not tested for API compatibility.
