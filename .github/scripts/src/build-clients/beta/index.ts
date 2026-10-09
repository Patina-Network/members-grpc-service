import { GitHubClient, VersioningClient, VersionUpdatingStrategy } from "@tahminator/pipeline";
import yargs from "yargs";
import { hideBin } from "yargs/helpers";

import { GITHUB_OWNER, GITHUB_REPOSITORY, requiredEnv, shortSha } from "../../consts";
import { CLIENT_PACKAGE_NAME, JAVA_GROUP_ID, PACKAGE_REGISTRY_URL } from "../../consts";
import { compileClients } from "../compile";

const { sha, prId } = await yargs(hideBin(process.argv))
  .option("sha", {
    type: "string",
    describe: "Full commit SHA the beta clients are built from",
    demandOption: true,
  })
  .option("prId", {
    type: "number",
    describe: "Pull request to comment the published versions on",
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

  const betaVersion = await new VersioningClient(
    ghClient,
    VersionUpdatingStrategy.RUST_CARGO,
  ).nextBeta(shortSha(sha));
  const goVersion = `v${betaVersion.replace(/^v/, "")}`;

  await compileClients(betaVersion);

  await ghClient.sendPrMessage({
    prId,
    owner: GITHUB_OWNER,
    repository: GITHUB_REPOSITORY,
    message: `The gRPC client packages have been uploaded to ${PACKAGE_REGISTRY_URL} under the following version(s):

- \`${betaVersion}\`

#### Rust:

\`\`\`toml
[dependencies]
${CLIENT_PACKAGE_NAME} = { version = "${betaVersion}", registry = "patina" }
\`\`\`

#### Go:

\`\`\`go
require patinanetwork.org/grpc/${CLIENT_PACKAGE_NAME} ${goVersion}
\`\`\`

#### Java:

\`\`\`xml
<dependency>
  <groupId>${JAVA_GROUP_ID}</groupId>
  <artifactId>${CLIENT_PACKAGE_NAME}</artifactId>
  <version>${betaVersion}</version>
</dependency>
\`\`\`
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
