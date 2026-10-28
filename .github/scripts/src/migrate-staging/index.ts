import { GitHubClient } from "@tahminator/pipeline";
import { $ } from "bun";
import { appendFileSync } from "node:fs";

import { GITHUB_OWNER, GITHUB_REPOSITORY, requiredEnv } from "../consts";

const CHECK_NAME = "Migrate staging DB";
const DISPATCH_ACTOR = "pat-agent[bot]";
const SHA = /^[0-9a-f]{40}$/;

/** Record one Actions step output. Values are single-line tokens. */
function setOutput(name: string, value: string) {
  appendFileSync(requiredEnv("GITHUB_OUTPUT"), `${name}=${value}\n`);
}

/** App client used for the team lookup and the pull request check run. */
function githubClient() {
  return GitHubClient.createWithGithubAppToken({
    appId: requiredEnv("_GITHUB_APP_APP_ID"),
    installationId: requiredEnv("_GITHUB_APP_INSTALLATION_ID"),
    privateKey: requiredEnv("_GITHUB_APP_PEM_CONTENT"),
  });
}

async function main() {
  if (process.env.GITHUB_ACTOR !== DISPATCH_ACTOR) {
    throw new Error("This workflow can only be dispatched by pat-agent.");
  }
  if (process.env.ACTION === "report") {
    await report();
    return;
  }
  await gate();
}

/** Confirm the commenter is on the dino team and whether the pull request changes db/. */
async function gate() {
  const username = requiredEnv("AUTHOR");
  const sha = requiredEnv("PR_SHA");
  const runUrl = requiredEnv("RUN_URL");
  if (!SHA.test(sha)) {
    throw new Error("Pull request SHA must be a 40 character hex commit.");
  }

  const ghClient = await githubClient();
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
      summary: "Verifying the commenter is on @Patina-Network/dino.",
    },
  });
  if (!check) {
    throw new Error("Failed to create staging migration status check.");
  }
  setOutput("check_run_id", String(check.id));

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

    const hasDbChanges = await pullRequestChangesDatabase(sha);
    if (!hasDbChanges) {
      await ghClient.statusCheck({
        ...update,
        status: "completed",
        conclusion: "skipped",
        output: {
          title: "Migration skipped",
          summary: "No changes under db/ were found when compared with main.",
        },
      });
      setOutput("migrate", "false");
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
    setOutput("migrate", "true");
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

/** Return whether the pull request changes any file under db/ compared with main. */
async function pullRequestChangesDatabase(sha: string): Promise<boolean> {
  await $`git fetch --no-tags origin ${sha}`;
  const diffOutput = await $`git diff --name-only origin/main...${sha}`.text();
  return diffOutput.split("\n").some((file) => file.startsWith("db/"));
}

/** Finish the check run after the migration command exits. */
async function report() {
  const ghClient = await githubClient();
  const succeeded = process.env.RESULT === "success";
  await ghClient.statusCheck({
    owner: GITHUB_OWNER,
    repository: GITHUB_REPOSITORY,
    action: "update",
    checkRunId: Number(requiredEnv("CHECK_RUN_ID")),
    detailsUrl: requiredEnv("RUN_URL"),
    status: "completed",
    conclusion: succeeded ? "success" : "failure",
    output: succeeded
      ? {
          title: "Migration successful",
          summary: "Staging database migrations completed successfully.",
        }
      : {
          title: "Staging migration failed",
          summary: "Staging migration failed. See the workflow logs for details.",
        },
  });
}

main().catch((error: unknown) => {
  console.error(error);
  process.exit(1);
});
