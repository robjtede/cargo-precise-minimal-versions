# Candidate dependency build failure

Run from the repository root:

```sh
just fixture candidate-build-failed
```

Expected output includes:

```text
  widget 1.0.0: candidate could not be built
  widget 1.1.0: passed
```

Widget 1.0.0 contains an intentional `compile_error!`. The isolated dependency build fails before the tool can compile the library against it. This result cannot establish whether the library uses an API that is absent from 1.0.0.

Inspect the saved candidate build diagnostics to identify the build error. In this fixture, select the working 1.1.0 release. [Cargo.toml.suggested](Cargo.toml.suggested) excludes the broken candidate:

```toml
widget = { version = "1.1.0", path = "baseline" }
```

The suggested-manifest test probes 1.1.0 and passes. The higher bound is a manual correction based on the intentional build error. The tool does not treat a candidate build failure as proof of an API minimum, so the original sampled report has no version recommendation.
