import { DockerClient } from "@tahminator/pipeline";
import yargs from "yargs";
import { hideBin } from "yargs/helpers";

import { ARCHITECTURES, dockerRepository, requiredEnv } from "../consts";

const { originalTag, newGithubTag, arch } = await yargs(hideBin(process.argv))
  .option("originalTag", {
    type: "string",
    describe: "Existing image tag to promote, e.g. abcd1234",
    demandOption: true,
  })
  .option("newGithubTag", {
    type: "string",
    describe: "Release tag to add, e.g. 1.2.34",
    demandOption: true,
  })
  .option("arch", {
    choices: ARCHITECTURES,
    describe: "Image architecture to promote. Must match the runner's architecture",
    default: "amd64" as const,
  })
  .strict()
  .parse();

async function main() {
  await using dockerClient = await DockerClient.create(
    requiredEnv("DOCKER_HUB_USERNAME"),
    requiredEnv("DOCKER_HUB_PAT"),
  );

  await dockerClient.promoteDockerImage({
    originalTag,
    newGithubTags: [newGithubTag, "latest"],
    repository: dockerRepository(arch),
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
