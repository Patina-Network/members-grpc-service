# Members gRPC Service

gRPC service that owns Patina Network member records.

- `proto/v1/members.proto`: `members.MembersService`, read-only lookups for any Patina app.
- `proto/v1/members_admin.proto`: `members.admin.MembersAdminService`, single and bulk write operations for core-admin only.

Both services ship in a single generated client package (`members-grpc-service`).

## Quickstart

Requires Rust 1.94 or newer.

```bash
# if you don't have cargo and/or rust on your machine yet
# mac users can run this
brew install rustup
# windows users can run this
winget install -e --id Rustlang.Rustup

# install sqlx-cli with cargo
cargo install sqlx-cli --no-default-features --features rustls,postgres

cargo build
cargo run # runs http on :3000, grpc on :50051
```

## Manual Testing

`bruno/` holds a [Bruno](https://www.usebruno.com/) collection for calling the service locally and on staging.

## Configuration

- `ENABLE_CORE_ADMIN_SERVICE`: `true` to mount `MembersAdminService`, default `false`.
- `GRPC_REFLECTION`: `true` to serve gRPC reflection (lists only mounted services), default `false`.
- `MAX_BULK_OPERATIONS`: maximum operations per `bulkMemberOperations` request, default `10000`.

`example.env` enables reflection and the core admin service for local development.

## Database development

Copy `example.env` to `.env` and set your local PostgreSQL credentials. Local
commands load that file explicitly with dotenvx:

```bash
just migrate
just seed
just dev
```

Staging uses `just migrate-stg`, production uses `just migrate-prod`, and CI
uses `just migrate-ci`. Only local development (`seed`) and CI (`seed-ci`) have
seed commands. Staging, production, and CI migration commands use exported
`DATABASE_*` variables and never load env files or fall back to example credentials. Migration commands accept arguments,
for example `just migrate down 1`.

Cargo uses SQLx offline mode by default, so builds, tests, and linting need no
running database. After changing SQL or migrations, run `just sqlx-prepare-local`
(with sqlx-cli 0.9.0 installed) and commit `.sqlx`. CI starts a fresh
`LocalPostgresClient`, applies `db/all`, generates the ignored SQLx metadata with
`cargo sqlx prepare`, and runs Rust checks with `SQLX_OFFLINE=false`.
Image builds depend on that validation and use the validated offline cache.

Member IDs are positive PostgreSQL `INTEGER` identities / protobuf `int32`.
Migration 0002 preserves member data but replaces UUIDs with new integer IDs;
clients must regenerate from the updated protos. Its rollback creates synthetic
UUIDs and does not restore the original IDs. Pagination tokens are readable
`0:123` (active members) or `1:123` (including archived members); pass the returned
token unchanged and keep `include_archived` the same between pages.
