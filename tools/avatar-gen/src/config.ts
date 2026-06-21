export interface Config {
  geminiApiKey: string;
  modelId: string;
  s3Endpoint: string;
  s3Region: string;
  s3Bucket: string;
  s3AccessKeyId: string;
  s3SecretAccessKey: string;
  giteaBaseUrl: string;
  giteaToken: string;
}

const REQUIRED = [
  "GEMINI_API_KEY",
  "SEAWEEDFS_S3_ENDPOINT",
  "SEAWEEDFS_S3_ACCESS_KEY_ID",
  "SEAWEEDFS_S3_SECRET_ACCESS_KEY",
  "GITEA_BASE_URL",
  "GITEA_TOKEN",
] as const;

export function loadConfig(env: Record<string, string | undefined>): Config {
  const missing = REQUIRED.filter((k) => !env[k]);
  if (missing.length) {
    // names only — never echo secret VALUES
    throw new Error(`missing required env: ${missing.join(", ")}`);
  }
  return {
    geminiApiKey: env.GEMINI_API_KEY!,
    modelId: env.AVATAR_MODEL_ID ?? "gemini-3-pro-image-preview",
    s3Endpoint: env.SEAWEEDFS_S3_ENDPOINT!,
    s3Region: env.SEAWEEDFS_S3_REGION ?? "us-east-1",
    s3Bucket: env.SEAWEEDFS_S3_BUCKET ?? "orrery-agent-sprites",
    s3AccessKeyId: env.SEAWEEDFS_S3_ACCESS_KEY_ID!,
    s3SecretAccessKey: env.SEAWEEDFS_S3_SECRET_ACCESS_KEY!,
    giteaBaseUrl: env.GITEA_BASE_URL!,
    giteaToken: env.GITEA_TOKEN!,
  };
}

export interface GiteaConfig {
  modelId: string;
  giteaBaseUrl: string;
  giteaToken: string;
}

const GITEA_REQUIRED = ["GITEA_BASE_URL", "GITEA_TOKEN"] as const;

export function loadGiteaConfig(env: Record<string, string | undefined>): GiteaConfig {
  const missing = GITEA_REQUIRED.filter((k) => !env[k]);
  if (missing.length) {
    // names only — never echo secret VALUES
    throw new Error(`missing required env: ${missing.join(", ")}`);
  }
  return {
    modelId: env.AVATAR_MODEL_ID ?? "gemini-3-pro-image-preview",
    giteaBaseUrl: env.GITEA_BASE_URL!,
    giteaToken: env.GITEA_TOKEN!,
  };
}
