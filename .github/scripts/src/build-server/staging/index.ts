import { DockerClient, GitHubClient } from "@tahminator/pipeline";
import yargs from "yargs";
import { hideBin } from "yargs/helpers";

import {
  ARCHITECTURES,
  dockerRepository,
  GITHUB_OWNER,
  GITHUB_REPOSITORY,
  requiredEnv,
  shortSha,
} from "../../consts";

const { sha, prId, arch } = await yargs(hideBin(process.argv))
  .option("sha", {
    type: "string",
    describe: "Full commit SHA the image is built from",
    demandOption: true,
  })
  .option("prId", {
    type: "number",
    describe: "Pull request to comment the pushed tags on",
    demandOption: true,
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
  const dockerHubUsername = requiredEnv("DOCKER_HUB_USERNAME");
  const repository = dockerRepository(arch);
  const image = `${dockerHubUsername}/${repository}`;
  const tags = [`staging-${shortSha(sha)}`];

  await using dockerClient = await DockerClient.create(
    dockerHubUsername,
    requiredEnv("DOCKER_HUB_PAT"),
  );

  await dockerClient.buildImage({
    dockerRepository: repository,
    dockerFileLocation: "Dockerfile",
    tags,
    platforms: [`linux/${arch}`],
  });

  const ghClient = await GitHubClient.createWithGithubAppToken({
    appId: requiredEnv("_GITHUB_APP_APP_ID"),
    installationId: requiredEnv("_GITHUB_APP_INSTALLATION_ID"),
    privateKey: requiredEnv("_GITHUB_APP_PEM_CONTENT"),
  });
  await ghClient.sendPrMessage({
    prId,
    owner: GITHUB_OWNER,
    repository: GITHUB_REPOSITORY,
    message: `The gRPC server image has been uploaded to https://hub.docker.com/r/${image}/tags under the following tags:

${tags.map((t) => `- \`${repository}:${t}\``).join("\n")}
`,
  });
}

main()
  .then(() => {
    process.exit(0);
  })
  .catch((e) => {
    console.error(e);
    process.exit(1);
  });
