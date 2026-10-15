set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

test *args:
    cargo test --locked {{ args }}

lint:
    cargo fmt --check
    cargo clippy --locked --all-targets -- -D warnings

fmt:
    cargo fmt
