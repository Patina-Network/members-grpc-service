set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

migrate *args:
    dotenvx run -- cargo run --locked --bin migrate {{ args }}

test *args:
    cargo test --locked {{ args }}

lint:
    cargo fmt --check
    cargo clippy --locked --all-targets -- -D warnings

fmt:
    cargo fmt
