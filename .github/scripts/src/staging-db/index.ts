import { GitHubClient } from "@tahminator/pipeline";
import { $ } from "bun";
import yargs from "yargs";
import { hideBin } from "yargs/helpers";

import { GITHUB_OWNER, GITHUB_REPOSITORY, requiredEnv } from "../consts";

const TEAM_SLUG = "dino";

export async function runStagingCommand(command: "migrate" | "clear") {
  const clearing = command === "clear";
  const operation = clearing ? "reset" : "migration";
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
    name: clearing ? "Clear staging DB" : "Migrate staging DB",
    status: "in_progress",
    detailsUrl: runUrl,
    output: {
      title: "Checking authorization",
      summary: `Verifying the commenter is on @${GITHUB_OWNER}/${TEAM_SLUG}.`,
    },
  });
  if (!check) {
    throw new Error(`Failed to create staging ${operation} status check.`);
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
        `Only members of @${GITHUB_OWNER}/${TEAM_SLUG} can ${command} the staging database.`,
      );
    }

    if (!clearing && !(await changesDatabase())) {
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
        title: clearing ? "Clearing staging database" : "Running staging migrations",
        summary: clearing
          ? "Deleting all staging data and reapplying the pull request migrations to members-stg."
          : "Applying pending migrations from the pull request to members-stg.",
      },
    });

    if (clearing) {
      await $`just clear-stg`;
    } else {
      await $`just migrate-stg up`;
    }

    await ghClient.statusCheck({
      ...update,
      status: "completed",
      conclusion: "success",
      output: {
        title: clearing ? "Reset successful" : "Migration successful",
        summary: clearing
          ? "Staging data cleared and PR migrations applied successfully."
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
          title: `Staging ${operation} failed`,
          summary: `Staging ${operation} failed. See the workflow logs for details.`,
        },
      })
      .catch((statusError: unknown) => {
        console.error(`Failed to report ${operation} failure:`, statusError);
      });

    throw error;
  }
}

async function changesDatabase(): Promise<boolean> {
  const diffOutput = await $`git diff --name-only origin/main...HEAD`.text();

  return diffOutput.split("\n").some((file) => file.startsWith("db/"));
}
