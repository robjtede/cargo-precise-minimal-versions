# Shared dependency type conflict

Run from the repository root:

```sh
just fixture shared-types
```

Expected output includes:

```text
  widget 1.0.0: inconclusive
  widget 1.1.0: inconclusive
```

The library returns `bridge::value()` as `widget::Shared`. The baseline graph uses one Widget build for both dependencies. During a probe, `bridge` retains that build while the library's direct Widget dependency is replaced by a separately compiled candidate.

Rust treats types from distinct crate builds as distinct types. The 1.0.0 probe has an E0308 type mismatch even though both Widget sources define `Shared`. This does not establish that the library needs a newer Widget API.

Verify this case with a normal Cargo build, which preserves the unified dependency graph. That build already passes for this fixture. Keep the dependency declaration unchanged, as recorded in [Cargo.toml.suggested](Cargo.toml.suggested):

```toml
widget = { version = "1", path = "baseline" }
```

The suggested-manifest test must remain inconclusive and make no version recommendation. Making this isolated probe conclusive requires the tool to preserve the shared type identity across both dependencies. A manifest change does not repair that probe limitation.
