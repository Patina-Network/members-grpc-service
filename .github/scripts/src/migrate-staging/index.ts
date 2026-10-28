import { GitHubClient } from "@tahminator/pipeline";
import { $ } from "bun";
import yargs from "yargs";
import { hideBin } from "yargs/helpers";

import { GITHUB_OWNER, GITHUB_REPOSITORY, requiredEnv } from "../consts";

const { runUrl, sha, username } = await yargs(hideBin(process.argv))
  .option("runUrl", {
    type: "string",
    describe: "Workflow run URL for the status check",
    demandOption: true,
  })
  .option("sha", {
    type: "string",
    describe: "PR head commit SHA",
    demandOption: true,
  })
  .option("username", {
    type: "string",
    describe: "Username of the person who triggered the command",
    demandOption: true,
  })
  .strict()
  .parse();

const CHECK_NAME = "Migrate staging DB";

async function main() {
  const ghClient = await GitHubClient.createWithGithubAppToken({
    appId: requiredEnv("_GITHUB_APP_APP_ID"),
    installationId: requiredEnv("_GITHUB_APP_INSTALLATION_ID"),
    privateKey: requiredEnv("_GITHUB_APP_PEM_CONTENT"),
  });

  const repo = { owner: GITHUB_OWNER, repository: GITHUB_REPOSITORY };
  const check = await ghClient.statusCheck({
    ...repo,
    action: "create",
    sha,
    name: CHECK_NAME,
    status: "in_progress",
    detailsUrl: runUrl,
    output: {
      title: "Checking authorization",
      summary: "Verifying the caller is on @Patina-Network/dino.",
    },
  });
  if (!check) {
    throw new Error("Failed to create staging migration status check.");
  }
  const update = {
    ...repo,
    action: "update" as const,
    checkRunId: check.id,
    detailsUrl: runUrl,
  };

  try {
    const isTeamMember = await ghClient.isTeamMember({
      org: GITHUB_OWNER,
      teamSlug: "dino",
      username,
    });
    if (!isTeamMember) {
      throw new Error("Only members of @Patina-Network/dino can migrate the staging database.");
    }

    await ghClient.statusCheck({
      ...update,
      status: "in_progress",
      output: {
        title: "Running staging migrations",
        summary: "Checking database changes and applying pending migrations to members-stg.",
      },
    });

    const result = await migrateStaging(sha);
    await ghClient.statusCheck({
      ...update,
      status: "completed",
      conclusion: result,
      output: {
        title: result === "skipped" ? "Migration skipped" : "Migration successful",
        summary:
          result === "skipped"
            ? "No changes under db/ were found when compared with main."
            : "Staging database migrations completed successfully.",
      },
    });
  } catch (error) {
    await ghClient
      .statusCheck({
        ...update,
        status: "completed",
        conclusion: "failure",
        output: {
          title: "Staging migration failed",
          summary: "Staging migration failed. See the workflow logs for details.",
        },
      })
      .catch((statusError: unknown) => {
        console.error("Failed to report migration failure:", statusError);
      });
    throw error;
  }
}

async function migrateStaging(headSha: string): Promise<"success" | "skipped"> {
  await $`git fetch origin main:main`;
  const diffOutput = await $`git diff --name-only main...${headSha}`.text();
  const hasDbChanges = diffOutput.split("\n").some((file) => file.startsWith("db/"));
  if (!hasDbChanges) {
    console.log("No db/ changes compared with main. Skipping staging migration.");
    return "skipped";
  }
  await $`just migrate-stg up`;
  return "success";
}

main()
  .then(() => {
    process.exit(0);
  })
  .catch((error: unknown) => {
    console.error(error);
    process.exit(1);
  });
