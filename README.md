# Members gRPC Service

gRPC service that owns Patina Network member records.

- `proto/v1/members.proto`: `members.MembersService`, read-only lookups for any Patina app.
- `proto/v1/members_admin.proto`: `members.admin.MembersAdminService`, single and bulk write operations for core-admin only.

Both services ship in a single generated client package (`members-grpc-service`).

## Quickstart

```bash
# if you don't have cargo and/or rust on your machine yet
# mac users can run this
brew install rustup
# windows users can run this
winget install -e --id Rustlang.Rustup

cargo build
cargo run # runs http on :3000, grpc on :50051
```

## Manual Testing

`bruno/` holds a [Bruno](https://www.usebruno.com/) collection for calling the service locally and on staging.
