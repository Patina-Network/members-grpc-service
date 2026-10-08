import {
  ProtobufCompilerBackend,
  ProtobufCompilerClient,
  ProtobufSourceLanguage,
  ProtobufTargetLanguage,
} from "@tahminator/pipeline";

import { requiredEnv } from "../consts";
import {
  CLIENT_PACKAGE_NAME,
  JAVA_GROUP_ID,
  PACKAGE_REGISTRY_URL,
} from "../consts";

/** Generates the Rust, Go and Java clients at `version` and publishes them. */
export async function compileClients(version: string) {
  await new ProtobufCompilerClient().compile({
    sourceLanguage: ProtobufSourceLanguage.RUST,
    backend: {
      type: ProtobufCompilerBackend.ARTIFACT_KEEPER,
      url: PACKAGE_REGISTRY_URL,
      username: requiredEnv("ARTIFACTKEEPER_USERNAME"),
      token: requiredEnv("ARTIFACTKEEPER_TOKEN"),
    },
    targetLanguages: {
      [ProtobufTargetLanguage.RUST]: {
        crateName: CLIENT_PACKAGE_NAME,
        version,
      },
      [ProtobufTargetLanguage.GO]: {
        version,
      },
      [ProtobufTargetLanguage.JAVA]: {
        groupId: JAVA_GROUP_ID,
        artifactId: CLIENT_PACKAGE_NAME,
        version,
        buildTool: "maven",
      },
    },
    protoFilesLocation: "./proto",
    bufToken: requiredEnv("BUF_TOKEN"),
  });
}
