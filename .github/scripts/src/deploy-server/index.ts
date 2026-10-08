import { GitHubClient } from "@tahminator/pipeline";
import yargs from "yargs";
import { hideBin } from "yargs/helpers";

import {
  dockerRepository,
  GITHUB_OWNER,
  GITHUB_REPOSITORY,
  K8S_MANIFESTS_REPOSITORY,
  requiredEnv,
} from "../consts";

const { newTagVersion } = await yargs(hideBin(process.argv))
  .option("newTagVersion", {
    type: "string",
    describe: "Release tag to deploy to production, e.g. 1.2.34",
    demandOption: true,
  })
  .strict()
  .parse();

async function main() {
  const ghClient = await GitHubClient.createWithGithubAppToken({
    appId: requiredEnv("_GITHUB_APP_APP_ID"),
    installationId: requiredEnv("_GITHUB_APP_INSTALLATION_ID"),
    privateKey: requiredEnv("_GITHUB_APP_PEM_CONTENT"),
  });

  // staging/production node pools are arm64, so the manifests run the -arm image
  await ghClient.updateK8sTagWithPR({
    manifestRepo: [GITHUB_OWNER, K8S_MANIFESTS_REPOSITORY],
    originRepo: [GITHUB_OWNER, GITHUB_REPOSITORY],
    kustomizationFilePath: `base/production/${dockerRepository()}/kustomization.yaml`,
    imageName: `${requiredEnv("DOCKER_HUB_USERNAME")}/${dockerRepository("arm64")}`,
    newTag: newTagVersion,
    environment: "production",
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
