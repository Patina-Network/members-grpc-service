import { GitHubClient } from "@tahminator/pipeline";
import yargs from "yargs";
import { hideBin } from "yargs/helpers";

import { GITHUB_OWNER, GITHUB_REPOSITORY, requiredEnv } from "../consts";

const { prId } = await yargs(hideBin(process.argv))
  .option("prId", {
    type: "number",
    describe: "Pull request number",
    demandOption: true,
  })
  .strict()
  .parse();

const githubClient = await GitHubClient.createWithGithubAppToken({
  appId: requiredEnv("_GITHUB_APP_APP_ID"),
  installationId: requiredEnv("_GITHUB_APP_INSTALLATION_ID"),
  privateKey: requiredEnv("_GITHUB_APP_PEM_CONTENT"),
});

await githubClient.sendPrMessage({
  prId,
  owner: GITHUB_OWNER,
  repository: GITHUB_REPOSITORY,
  message: `
### Available PR Commands

- \`/migrate\` - Applies this PR's database migrations to the staging DB (can only be run by someone on \`@Patina-Network/dino\`).
`.trim(),
});
