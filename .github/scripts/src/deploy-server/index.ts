import { type Environment, GitHubClient } from "@tahminator/pipeline";
import yargs from "yargs";
import { hideBin } from "yargs/helpers";

import {
  dockerRepository,
  GITHUB_OWNER,
  GITHUB_REPOSITORY,
  K8S_MANIFESTS_REPOSITORY,
  requiredEnv,
  shortSha,
} from "../consts";

const DEPLOYMENTS = ["members-grpc-service", "members-admin-grpc-service"] as const;

const { environment, releaseTag, sha } = await yargs(hideBin(process.argv))
  .option("environment", {
    choices: ["staging", "production"] satisfies Environment[],
    describe: "staging images are tagged staging-<sha>; production are <sha>",
    default: "production" as Environment,
  })
  .option("releaseTag", {
    type: "string",
    describe: "Deploy the version named by a release tag",
  })
  .option("sha", {
    type: "string",
    describe: "Deploy the image built from this commit",
  })
  .check(({ environment, releaseTag, sha }) => {
    if (!releaseTag === !sha) {
      throw new Error("Pass exactly one non-empty --releaseTag or --sha");
    }
    if (environment === "staging" && releaseTag) {
      throw new Error("--releaseTag can only be deployed to production");
    }
    return true;
  })
  .strict()
  .parse();

async function main() {
  const version =
    environment === "staging"
      ? `staging-${shortSha(sha ?? "")}`
      : (releaseTag ?? shortSha(sha ?? ""));

  const ghClient = await GitHubClient.createWithGithubAppToken({
    appId: requiredEnv("_GITHUB_APP_APP_ID"),
    installationId: requiredEnv("_GITHUB_APP_INSTALLATION_ID"),
    privateKey: requiredEnv("_GITHUB_APP_PEM_CONTENT"),
  });

  const imageName = `${requiredEnv("DOCKER_HUB_USERNAME")}/${dockerRepository("arm64")}`;

  for (const deployment of DEPLOYMENTS) {
    await ghClient.updateK8sTagWithPR({
      manifestRepo: [GITHUB_OWNER, K8S_MANIFESTS_REPOSITORY],
      originRepo: [GITHUB_OWNER, GITHUB_REPOSITORY],
      kustomizationFilePath: `base/${environment}/${deployment}/kustomization.yaml`,
      imageName,
      newTag: version,
      environment,
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
