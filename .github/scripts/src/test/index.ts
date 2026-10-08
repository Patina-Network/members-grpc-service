import { SonarScannerClient } from "@tahminator/pipeline";
import { $ } from "bun";

import { exclusions } from "../../../../exclusions";
import { requiredEnv, SONAR_ORGANIZATION, SONAR_PROJECT_KEY } from "../consts";

async function main() {
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
      sourceCodeDir: "./",
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
