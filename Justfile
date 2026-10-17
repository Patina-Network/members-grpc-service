set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

migrate *args="up":
    dotenvx run -f .env -- just _migrate {{ args }}

migrate-stg *args="up":
    @just _migrate {{ args }}

migrate-prod *args="up":
    @just _migrate {{ args }}

migrate-ci *args="up":
    @just _migrate {{ args }}

seed *args="up":
    dotenvx run -f .env -- just _seed {{ args }}

seed-stg *args="up":
    @just _seed {{ args }}

seed-prod *args="up":
    @just _seed {{ args }}

seed-ci *args="up":
    @just _seed {{ args }}

_migrate *args="up":
    @PGHOST="$DATABASE_HOST" PGPORT="$DATABASE_PORT" \
        PGDATABASE="$DATABASE_NAME" PGUSER="$DATABASE_USER" \
        PGPASSWORD="$DATABASE_PASSWORD" PGSSLMODE="${DATABASE_SSLMODE:-require}" \
        migrate -source file://db/all -database 'pgx5:///?x-migrations-table=schema_migrations' -verbose {{ args }}

_seed *args="up":
    @PGHOST="$DATABASE_HOST" PGPORT="$DATABASE_PORT" \
        PGDATABASE="$DATABASE_NAME" PGUSER="$DATABASE_USER" \
        PGPASSWORD="$DATABASE_PASSWORD" PGSSLMODE="${DATABASE_SSLMODE:-require}" \
        migrate -source file://db/seed -database 'pgx5:///?x-migrations-table=seed_migrations' -verbose {{ args }}

dev:
    dotenvx run -f .env -- bash -c 'GRPC_REFLECTION="${GRPC_REFLECTION:-true}" cargo run --locked'

test *args:
    cargo test --locked {{ args }}

lint:
    cargo fmt --check
    cargo clippy --locked --all-targets -- -D warnings

fmt:
    cargo fmt

sqlx-prepare-local: migrate
    dotenvx run -f .env -- just sqlx-prepare-env

sqlx-prepare-env:
    @PGHOST="$DATABASE_HOST" PGPORT="$DATABASE_PORT" \
        PGDATABASE="$DATABASE_NAME" PGUSER="$DATABASE_USER" \
        PGPASSWORD="$DATABASE_PASSWORD" PGSSLMODE="${DATABASE_SSLMODE:-require}" \
        DATABASE_URL='postgresql:///' SQLX_OFFLINE=false cargo sqlx prepare -- --locked --all-targets
