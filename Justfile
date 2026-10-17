set dotenv-load := true
set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

migrate *args="up":
    @if [ -z "${DATABASE_HOST:-}" ]; then set -a; . ./example.env; set +a; fi; \
      PGHOST="$DATABASE_HOST" PGPORT="$DATABASE_PORT" \
        PGDATABASE="$DATABASE_NAME" PGUSER="$DATABASE_USER" \
        PGPASSWORD="$DATABASE_PASSWORD" PGSSLMODE="${DATABASE_SSLMODE:-require}" \
        migrate -source file://db/migration -database pgx5:/// -verbose {{ args }}; \
      case "{{ args }}" in \
        up) \
          if [ "${APPLY_REPEATED_MIGRATIONS:-true}" = "true" ]; then \
            set -- db/repeated/*.sql; \
            for file in "$@"; do \
              PGHOST="$DATABASE_HOST" PGPORT="$DATABASE_PORT" \
                PGDATABASE="$DATABASE_NAME" PGUSER="$DATABASE_USER" \
                PGPASSWORD="$DATABASE_PASSWORD" PGSSLMODE="${DATABASE_SSLMODE:-require}" \
                psql -v ON_ERROR_STOP=1 --single-transaction -f "$file"; \
            done; \
          fi ;; \
      esac

dev:
    cargo run --locked

test *args:
    cargo test --locked {{ args }}

lint:
    cargo fmt --check
    cargo clippy --locked --all-targets -- -D warnings

fmt:
    cargo fmt
