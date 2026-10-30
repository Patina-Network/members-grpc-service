import { GitHubClient } from "@tahminator/pipeline";
import { $ } from "bun";
import yargs from "yargs";
import { hideBin } from "yargs/helpers";

import { GITHUB_OWNER, GITHUB_REPOSITORY, requiredEnv } from "../consts";

const CHECK_NAME = "Migrate staging DB";
const TEAM_SLUG = "dino";

const { author, runUrl } = await yargs(hideBin(process.argv))
  .option("author", {
    type: "string",
    describe: "Username of the person who triggered the command",
    demandOption: true,
  })
  .option("runUrl", {
    type: "string",
    describe: "Workflow run URL linked from the status check",
    demandOption: true,
  })
  .strict()
  .parse();

async function main() {
  const sha = (await $`git rev-parse HEAD`.text()).trim();

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
      summary: `Verifying the commenter is on @${GITHUB_OWNER}/${TEAM_SLUG}.`,
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
      teamSlug: TEAM_SLUG,
      username: author,
    });
    if (!isTeamMember) {
      throw new Error(
        `Only members of @${GITHUB_OWNER}/${TEAM_SLUG} can migrate the staging database.`,
      );
    }

    if (!(await changesDatabase())) {
      await ghClient.statusCheck({
        ...update,
        status: "completed",
        conclusion: "skipped",
        output: {
          title: "Migration skipped",
          summary: "No changes under db/ were found when compared with main.",
        },
      });
      return;
    }

    await ghClient.statusCheck({
      ...update,
      status: "in_progress",
      output: {
        title: "Running staging migrations",
        summary: "Applying pending migrations from the pull request to members-stg.",
      },
    });

    await $`just migrate-stg up`;

    await ghClient.statusCheck({
      ...update,
      status: "completed",
      conclusion: "success",
      output: {
        title: "Migration successful",
        summary: "Staging database migrations completed successfully.",
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

/** Return whether the checked out commit changes any file under db/ compared with main. */
async function changesDatabase(): Promise<boolean> {
  const diffOutput = await $`git diff --name-only origin/main...HEAD`.text();

  return diffOutput.split("\n").some((file) => file.startsWith("db/"));
}

main()
  .then(() => {
    process.exit(0);
  })
  .catch((e: unknown) => {
    console.error(e);
    process.exit(1);
  });
