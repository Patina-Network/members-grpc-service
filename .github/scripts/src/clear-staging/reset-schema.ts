import { SQL } from "bun";

import { requiredEnv } from "../consts";

if (requiredEnv("DATABASE_NAME") !== "members-stg") {
  throw new Error("Refusing to clear a database other than members-stg");
}

const url = new URL("postgresql://localhost/members-stg");
url.hostname = requiredEnv("DATABASE_HOST");
url.port = requiredEnv("DATABASE_PORT");
url.username = requiredEnv("DATABASE_USER");
url.password = requiredEnv("DATABASE_PASSWORD");
url.searchParams.set("sslmode", process.env.DATABASE_SSLMODE ?? "require");

// Explicit options take precedence over inherited PG_DATABASE / PGDATABASE.
await using sql = new SQL(url.toString(), { database: "members-stg" });
await sql.begin(async (tx) => {
  await tx`
    DO $$
    DECLARE object record;
    BEGIN
      FOR object IN SELECT tablename FROM pg_tables WHERE schemaname = 'public'
      LOOP
        EXECUTE format('DROP TABLE IF EXISTS public.%I CASCADE', object.tablename);
      END LOOP;
      FOR object IN
        SELECT t.typname FROM pg_type t
        JOIN pg_namespace n ON n.oid = t.typnamespace
        WHERE n.nspname = 'public' AND t.typtype = 'e'
      LOOP
        EXECUTE format('DROP TYPE public.%I CASCADE', object.typname);
      END LOOP;
    END $$
  `;
});
