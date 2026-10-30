import { LocalPostgresClient } from "@tahminator/pipeline";
import { $ } from "bun";

async function main() {
  await using pgClient = await LocalPostgresClient.create({
    database: "members",
  });
  const { database, host, password, port, user } = pgClient.state;
  const databaseUrl = new URL(`postgresql://${host}:${port}/${database}`);
  databaseUrl.username = user;
  databaseUrl.password = password;
  databaseUrl.searchParams.set("sslmode", "disable");

  await $.env({
    ...process.env,
    DATABASE_HOST: host,
    DATABASE_PORT: String(port),
    DATABASE_NAME: database,
    DATABASE_USER: user,
    DATABASE_PASSWORD: password,
    DATABASE_SSLMODE: "disable",
    DATABASE_URL: databaseUrl.toString(),
    SQLX_OFFLINE: "false",
  })`just migrate-ci up && cargo sqlx prepare -- --locked --all-targets`;
}

void main();
