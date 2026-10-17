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
  })`just migrate-ci up && just migrate-ci up && just seed-ci up && just seed-ci up && just migrate-ci down 1 && just migrate-ci up && just seed-ci down -all && just migrate-ci down -all && just migrate-ci up && just seed-ci up`;
}

await main();
