import '.toolchain/rust.just'

toolchain := "stable"

_list:
    @just --list

# Check project formatting, workflows, and dependencies.
check: fmt-check clippy
    actionlint
    zizmor --offline .github/workflows
    cargo +{{ toolchain }} shear

# Format project files.
fmt:
    just --fmt
    nixfmt flake.nix
    fd --type=file --hidden --extension=md --extension=yml --extension=yaml --exec-batch prettier --write
    fd --type=file --hidden --extension=toml --extension=suggested --exec-batch taplo format
    cargo +nightly fmt --all
    fd --type=file --extension=rs . fixtures --exec-batch rustfmt +nightly --edition 2021

# Check project formatting.
fmt-check:
    just --fmt --check
    nixfmt --check flake.nix
    fd --type=file --hidden --extension=md --extension=yml --extension=yaml --exec-batch prettier --check
    fd --type=file --hidden --extension=toml --extension=suggested --exec-batch taplo format --check
    fd --type=file --hidden --extension=toml --extension=suggested --exec-batch taplo lint
    cargo +nightly fmt --all -- --check
    fd --type=file --extension=rs . fixtures --exec-batch rustfmt +nightly --edition 2021 --check

# Run tests. Nightly Cargo must also be installed for compiler probes.
test *args:
    cargo +{{ toolchain }} nextest run --all-targets --locked {{ args }}

# Run the pinned upstream regression with network access and Rust 1.88.0.
test-real-world:
    cargo +{{ toolchain }} nextest run --locked --test real_world -- --ignored

# Check all Rust targets with Clippy.
clippy:
    cargo +{{ toolchain }} clippy --all-targets --locked -- -D warnings

# Build the binary.
build *args:
    cargo +{{ toolchain }} build --locked {{ args }}

# Build all Rust targets with the declared MSRV.
build-msrv:
    cargo {{ msrv_rustup }} build --all-targets --locked

# Run the binary locally.
run *args:
    cargo +{{ toolchain }} run --locked -- {{ args }}

# Run a local diagnostic fixture without registry access.
fixture name *args:
    just --set toolchain {{ toolchain }} run --manifest-path fixtures/{{ name }}/Cargo.toml --dependency widget --candidate-path fixtures/{{ name }}/candidate --candidate-path fixtures/{{ name }}/baseline --search linear --offline {{ args }}
