import { LocalPostgresClient, SonarScannerClient } from "@tahminator/pipeline";
import { $ } from "bun";

import { exclusions } from "../../../../exclusions";
import { requiredEnv, SONAR_ORGANIZATION, SONAR_PROJECT_KEY } from "../consts";

async function main() {
  await using pgClient = await LocalPostgresClient.create({
    database: "members",
  });

  const { database, host, port, password, user } = pgClient.state;
  const databaseUrl = new URL(`postgresql://${host}:${port}/${database}`);
  databaseUrl.username = user;
  databaseUrl.password = password;
  databaseUrl.searchParams.set("sslmode", "disable");

  $.env({
    ...process.env,
    DATABASE_HOST: host,
    DATABASE_PORT: String(port),
    DATABASE_NAME: database,
    DATABASE_USER: user,
    DATABASE_PASSWORD: password,
    DATABASE_SSLMODE: "disable",
    DATABASE_URL: databaseUrl.toString(),
    SQLX_OFFLINE: "false",
  });

  await $`just migrate-ci up`;
  await $`cargo sqlx prepare -- --locked --all-targets`;

  const sonarClient = new SonarScannerClient({
    auth: {
      token: requiredEnv("SONAR_TOKEN"),
    },
    scan: {
      additionalArgs: {
        "rust.lcov.reportPaths": "./lcov.info",
        exclusions: `${exclusions}`,
      },
      organization: SONAR_ORGANIZATION,
      sourceCodeDir: "src",
      projectKey: SONAR_PROJECT_KEY,
    },
    run: {
      runTestsCmd: $`cargo clippy --locked --all-targets --message-format=json > clippy-report.json && cargo tarpaulin --locked --out lcov`,
    },
  });

  await sonarClient.runTests();
  await sonarClient.uploadTestCoverage();
}

main()
  .then(() => {
    process.exit(0);
  })
  .catch((e) => {
    console.error(e);
    process.exit(1);
  });
