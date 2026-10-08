fmt:
    cargo +nightly fmt --all

fmt-check:
    cargo +nightly fmt --all -- --check

test *args:
    cargo +nightly test --locked {{args}}

clippy:
    cargo +nightly clippy --all-targets --locked -- -D warnings

