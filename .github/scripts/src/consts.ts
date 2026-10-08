export const GITHUB_OWNER = "Patina-Network";
export const GITHUB_REPOSITORY = "hello-world-grpc-service";

// k8s repo name that is used to deploy versions against
export const K8S_MANIFESTS_REPOSITORY = "k8s-manifests";

export const SONAR_ORGANIZATION = "patina-network";
export const SONAR_PROJECT_KEY = `${GITHUB_OWNER}_${GITHUB_REPOSITORY}`;

export const ARCHITECTURES = ["amd64", "arm64"] as const;
export type Architecture = (typeof ARCHITECTURES)[number];

const DOCKER_REPOSITORY = "hello-world-grpc-service";
export function dockerRepository(arch: Architecture = "amd64") {
  return arch === "arm64" ? `${DOCKER_REPOSITORY}-arm` : DOCKER_REPOSITORY;
}

// used for grpc clients
export const PACKAGE_REGISTRY_URL = "https://pkg.vpn.patinanetwork.org";
export const CLIENT_PACKAGE_NAME = "hello-world-grpc-service";
export const JAVA_GROUP_ID = "org.patinanetwork.grpc";

export function shortSha(sha: string) {
  const full = sha.trim();
  if (!/^[0-9a-f]{40}$/.test(full)) {
    throw new Error(`Expected a full commit SHA, got "${full}"`);
  }
  return full.slice(0, 8);
}

export function requiredEnv(name: string) {
  const v = process.env[name];
  if (!v) {
    throw new Error(`Missing ${name} from env`);
  }
  return v;
}
