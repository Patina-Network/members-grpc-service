import {
  GitHubClient,
  VersioningClient,
  VersionUpdatingStrategy,
} from "@tahminator/pipeline";

import { requiredEnv } from "../consts";

async function main() {
  const ghClient = await GitHubClient.createWithGithubAppToken({
    appId: requiredEnv("_GITHUB_APP_APP_ID"),
    installationId: requiredEnv("_GITHUB_APP_INSTALLATION_ID"),
    privateKey: requiredEnv("_GITHUB_APP_PEM_CONTENT"),
  });

  const versioningClient = new VersioningClient(
    ghClient,
    VersionUpdatingStrategy.RUST_CARGO,
  );

  const cargoToml = Bun.TOML.parse(await Bun.file("./Cargo.toml").text()) as {
    package?: { version?: string };
  };
  const version = cargoToml.package?.version;
  if (!version) {
    throw new Error("Missing [package].version in Cargo.toml");
  }

  await ghClient.createTag({
    nextTag: await versioningClient.next(version),
    onPreTagCreate: async (tag) => {
      await versioningClient.update(tag);
    },
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
