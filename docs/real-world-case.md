# Verified upstream case: roxmltree and memchr

On 2026-10-08, the CLI found an underspecified dependency requirement in `roxmltree` 0.21.1. The test used the complete, unchanged upstream source at commit [`3f5c78dea57662b43a2f41961397b1f5ffe27166`](https://github.com/RazrFalcon/roxmltree/commit/3f5c78dea57662b43a2f41961397b1f5ffe27166).

The [upstream manifest](https://github.com/RazrFalcon/roxmltree/blob/3f5c78dea57662b43a2f41961397b1f5ffe27166/Cargo.toml#L28) declares:

```toml
memchr = { version = "2", default-features = false }
```

This requirement permits memchr 2.0.0. However, [the parser](https://github.com/RazrFalcon/roxmltree/blob/3f5c78dea57662b43a2f41961397b1f5ffe27166/src/parse.rs#L647-L648) uses `memchr_iter` to count possible nodes and attributes. The function is absent from [memchr 2.0.2](https://docs.rs/memchr/2.0.2/memchr/), and is present in [memchr 2.1.0](https://github.com/BurntSushi/memchr/blob/2.1.0/src/lib.rs#L60-L64).

## Compiler evidence

The run disabled roxmltree's default features and used Rust 1.88.0. The baseline resolved memchr 2.8.3. The direct feature check passed and found no missing features.

| memchr version | CLI outcome   | Compiler result                                |
| -------------- | ------------- | ---------------------------------------------- |
| 2.0.0          | `api_failure` | E0432: unresolved import `memchr::memchr_iter` |
| 2.0.1          | `api_failure` | Same error                                     |
| 2.0.2          | `api_failure` | Same error                                     |
| 2.1.0          | `passed`      | roxmltree library compiles                     |

The linear search checked every eligible release before the first pass. Its JSON report has `exhaustive: true`, `minimum_verified: true`, and `recommendation: "2.1.0"`. The original run saved its report and compiler output under `/private/tmp/cpmv-real-world-20261008.AQiogz/linear/`. See the [JSON report](/private/tmp/cpmv-real-world-20261008.AQiogz/linear/report.json).

The clone's manifests were unchanged. It had no Cargo.lock before or after the run.

## Feature scope

The verified API minimum of 2.1.0 applies to `--no-default-features`. It does not establish the minimum for roxmltree's default build.

The [roxmltree `std` feature](https://github.com/RazrFalcon/roxmltree/blob/3f5c78dea57662b43a2f41961397b1f5ffe27166/Cargo.toml#L19-L21) enables `memchr/std`. The [published registry records](https://github.com/rust-lang/crates.io-index/blob/master/me/mc/memchr) show that `std` first exists in memchr 2.3.0. Earlier releases use the name `use_std`. A manifest change that retains this feature forwarding must therefore require at least 2.3.0:

```toml
memchr = { version = "2.3", default-features = false }
```

A second run used roxmltree's default features and Rust 1.88.0. Its isolated candidate features were `["std"]`. Every eligible release from 2.0.0 through 2.2.1 had outcome `candidate_build_failed` because the candidate did not provide `std`. Version 2.3.0 passed. The [default-feature report](/private/tmp/cpmv-real-world-20261008.AQiogz/default-features/report.json) has `lowest_passing: "2.3.0"`, `minimum_verified: false`, and `recommendation: null`. The CLI does not use failed candidate builds as proof of a version minimum. The practical 2.3 requirement above comes from the feature definitions and the passing compiler probe.

## Reproduce the API check

Run these commands from this repository. Install nightly Cargo and the Rust 1.88.0 toolchain first.

```sh
case_root=$(mktemp -d /tmp/cpmv-roxmltree.XXXXXX)

git clone https://github.com/RazrFalcon/roxmltree.git "$case_root/roxmltree"
git -C "$case_root/roxmltree" checkout --detach \
  3f5c78dea57662b43a2f41961397b1f5ffe27166

just run \
  --manifest-path "$case_root/roxmltree/Cargo.toml" \
  --dependency memchr \
  --no-default-features \
  --rustc-toolchain 1.88.0 \
  --search linear \
  --json \
  --report-dir "$case_root/report"
```

Keep the upstream commit fixed for the regression test. Use a fresh temporary clone for each run. This preserves the real source that exposed the error, even if upstream corrects its manifest later.

The opt-in regression in `tests/real_world.rs` performs this check against the pinned upstream clone. It asserts all four probe outcomes, the verified minimum, the JSON recommendation, and that the manifest and lockfile are unchanged. It passed on 2026-10-08. Run it with:

```sh
just test-real-world
```
