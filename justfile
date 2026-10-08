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
    fd --type=file --hidden --extension=toml --exec-batch taplo format
    cargo +nightly fmt --all

# Check project formatting.
fmt-check:
    just --fmt --check
    nixfmt --check flake.nix
    fd --type=file --hidden --extension=md --extension=yml --extension=yaml --exec-batch prettier --check
    fd --type=file --hidden --extension=toml --exec-batch taplo format --check
    fd --type=file --hidden --extension=toml --exec-batch taplo lint
    cargo +nightly fmt --all -- --check

# Run tests. Nightly Cargo must also be installed for compiler probes.
test *args:
    cargo +{{ toolchain }} nextest run --all-targets --locked {{ args }}

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
