import { LocalPostgresClient } from "@tahminator/pipeline";
import { $, SQL } from "bun";

async function main() {
  await using pgClient = await LocalPostgresClient.create({
    database: "members-stg",
  });

  const { database, host, port, password, user } = pgClient.state;

  const env = {
    ...process.env,
    DATABASE_HOST: host,
    DATABASE_PORT: String(port),
    DATABASE_NAME: database,
    DATABASE_USER: user,
    DATABASE_PASSWORD: password,
    DATABASE_SSLMODE: "disable",
  };

  await $.env(
    env,
  )`just migrate-ci up && just migrate-ci up && just seed-ci up && just seed-ci up && just migrate-ci down 1 && just migrate-ci up && just seed-ci down -all && just migrate-ci down -all && just migrate-ci up && just seed-ci up`;

  const url = new URL(`postgresql://${host}:${port}/${database}`);
  url.username = user;
  url.password = password;
  url.searchParams.set("sslmode", "disable");
  await using sql = new SQL(url.toString());

  await sql`UPDATE schema_migrations SET dirty = true`;
  await sql`CREATE TABLE partial_migration (id integer)`;
  await $.env({
    ...env,
    PGDATABASE: "reset_wrong_database",
    PG_DATABASE: "",
  })`just clear-stg`;
  await $.env({
    ...env,
    PGDATABASE: "",
    PG_DATABASE: "reset_wrong_database",
  })`just clear-stg`;

  const [state] = await sql`
    SELECT
      (SELECT count(*)::integer FROM "Member") AS members,
      (SELECT dirty FROM schema_migrations) AS dirty,
      to_regclass('public.partial_migration') AS partial_table,
      to_regclass('public.seed_migrations') AS seed_table,
      EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'Member' AND column_name = 'school'
      ) AS has_school
  `;
  if (
    state.members !== 0 ||
    state.dirty !== false ||
    state.partial_table !== null ||
    state.seed_table !== null ||
    state.has_school !== true
  ) {
    throw new Error("Staging reset did not restore a clean, empty schema");
  }

  const refused = await $.env({ ...env, DATABASE_NAME: "members" })`just clear-stg`
    .quiet()
    .nothrow();
  if (refused.exitCode === 0 || !refused.stderr.toString().includes("Refusing to clear")) {
    throw new Error("Staging reset must reject any database other than members-stg");
  }
}

await main();
