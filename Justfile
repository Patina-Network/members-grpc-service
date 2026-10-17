set dotenv-load := true
set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

migrate *args="up":
    @PGHOST="$DATABASE_HOST" PGPORT="$DATABASE_PORT" \
      PGDATABASE="$DATABASE_NAME" PGUSER="$DATABASE_USER" \
      PGPASSWORD="$DATABASE_PASSWORD" PGSSLMODE="${DATABASE_SSLMODE:-require}" \
      migrate -source file://db/migration -database pgx5:/// -verbose {{ args }}

# Reapply db/repeated mock data. Local only. Staging and production do not run this.
# Uses .env when present, otherwise example.env.
seed:
    @if [ -z "${DATABASE_HOST:-}" ]; then set -a; . ./example.env; set +a; fi; \
      set -- db/repeated/*.sql; \
      for file in "$@"; do \
        PGHOST="$DATABASE_HOST" PGPORT="$DATABASE_PORT" \
          PGDATABASE="$DATABASE_NAME" PGUSER="$DATABASE_USER" \
          PGPASSWORD="$DATABASE_PASSWORD" PGSSLMODE="${DATABASE_SSLMODE:-require}" \
          psql -v ON_ERROR_STOP=1 --single-transaction -f "$file"; \
      done

dev:
    cargo run --locked

test *args:
    cargo test --locked {{ args }}

lint:
    cargo fmt --check
    cargo clippy --locked --all-targets -- -D warnings

fmt:
    cargo fmt
