import { LocalPostgresClient } from "@tahminator/pipeline";
import { $ } from "bun";

async function main() {
  await using pgClient = await LocalPostgresClient.create({
    database: "members",
  });

  const { database, host, port, password, user } = pgClient.state;

  await $.env({
    ...process.env,
    DATABASE_HOST: host,
    DATABASE_PORT: String(port),
    DATABASE_NAME: database,
    DATABASE_USER: user,
    DATABASE_PASSWORD: password,
    DATABASE_SSLMODE: "disable",
  })`just migrate up && just migrate up && just migrate down -all && just migrate up && cargo test --locked --test members_api postgres_member_reads -- --ignored`;
}

await main();
