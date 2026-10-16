set dotenv-load := true
set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

migrate *args="up":
    @PGHOST="$DATABASE_HOST" PGPORT="$DATABASE_PORT" \
      PGDATABASE="$DATABASE_NAME" PGUSER="$DATABASE_USER" \
      PGPASSWORD="$DATABASE_PASSWORD" PGSSLMODE="${DATABASE_SSLMODE:-require}" \
      migrate -source file://db/migration -database pgx5:/// -verbose {{ args }}

dev:
    cargo run --locked

test *args:
    cargo test --locked {{ args }}

lint:
    cargo fmt --check
    cargo clippy --locked --all-targets -- -D warnings

fmt:
    cargo fmt
