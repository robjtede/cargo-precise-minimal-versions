# Missing dependency API

Run from the repository root:

```sh
just fixture api-failure
```

Expected output includes:

```text
  widget 1.0.0: required API is unavailable
  widget 1.1.0: passed
```

The library calls `widget::needed`. Widget 1.0.0 does not provide that function, so the compiler probe fails with a missing-API error. Widget 1.1.0 provides it and passes.

Require the release that provides the API. [Cargo.toml.suggested](Cargo.toml.suggested) changes the dependency to:

```toml
widget = { version = "1.1.0", path = "baseline" }
```

This excludes the failing 1.0.0 candidate. The suggested-manifest test probes 1.1.0 and passes.

The version change is a manual correction based on the local sources. This sampled search does not verify a minimum version or emit a version recommendation. A real dependency needs an exhaustive check of earlier eligible releases before the tool can recommend a higher bound.
