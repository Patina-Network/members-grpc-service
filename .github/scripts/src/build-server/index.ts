import { DockerClient, GitHubClient } from "@tahminator/pipeline";
import { $ } from "bun";
import yargs from "yargs";
import { hideBin } from "yargs/helpers";

import { ARCHITECTURES, dockerRepository, requiredEnv, shortSha } from "../consts";

const { getGhaOutput, githubOutputFile, arch } = await yargs(hideBin(process.argv))
  .option("getGhaOutput", {
    type: "boolean",
    describe: "Enable GitHub Actions output to receive latest built tag version",
    default: false,
  })
  .option("githubOutputFile", {
    type: "string",
    describe: "Path to GITHUB_OUTPUT (passed in automatically in CI)",
    default: process.env.GITHUB_OUTPUT,
  })
  .option("arch", {
    choices: ARCHITECTURES,
    describe:
      "Target architecture, built natively on a matching runner. arm64 pushes to a separate -arm repository",
    default: "amd64" as const,
  })
  .strict()
  .parse();

async function main() {
  const short = shortSha(await $`git rev-parse HEAD`.text());
  const tags = ["latest", short];

  await using dockerClient = await DockerClient.create(
    requiredEnv("DOCKER_HUB_USERNAME"),
    requiredEnv("DOCKER_HUB_PAT"),
  );

  await dockerClient.buildImage({
    dockerRepository: dockerRepository(arch),
    dockerFileLocation: "Dockerfile",
    tags,
    platforms: [`linux/${arch}`],
  });

  if (getGhaOutput && githubOutputFile) {
    const ghClient = await GitHubClient.createWithGithubAppToken({
      appId: requiredEnv("_GITHUB_APP_APP_ID"),
      installationId: requiredEnv("_GITHUB_APP_INSTALLATION_ID"),
      privateKey: requiredEnv("_GITHUB_APP_PEM_CONTENT"),
    });
    await ghClient.outputToGithubOutput({
      overrideGithubOutputFile: githubOutputFile,
      ctx: { tag: short },
    });
  }
}

main()
  .then(() => {
    process.exit(0);
  })
  .catch((e) => {
    console.error(e);
    process.exit(1);
  });
