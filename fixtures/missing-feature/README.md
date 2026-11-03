# Missing direct dependency feature

Run from the repository root:

```sh
just fixture missing-feature
```

Expected output includes:

```text
Suggested direct feature: widget/extra
```

The library calls `widget::needed`, which requires Widget's `extra` feature. The `bridge` dependency enables that feature, so Cargo's combined dependency graph builds. The library's own Widget declaration omits it.

The tool isolates the features requested by the library. Its first feature check fails, then its recheck passes after adding `extra`. This verifies the feature recommendation. The original version probes remain inconclusive because they retain the missing feature declaration.

Request `extra` directly. [Cargo.toml.suggested](Cargo.toml.suggested) changes the dependency to:

```toml
widget = { version = "1", path = "baseline", default-features = false, features = ["extra"] }
```

The suggested-manifest test passes the feature check and the Widget 1.0.0 probe. Keep the version requirement at `1`; the missing feature does not require a higher version bound.

With `--check`, the original manifest exits with status 3 for the verified missing feature. The suggested manifest exits with status 0.
