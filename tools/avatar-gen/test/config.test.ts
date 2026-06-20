import { describe, it, expect } from "vitest";
import { loadConfig } from "../src/config.js";

const full = {
  GEMINI_API_KEY: "g",
  SEAWEEDFS_S3_ENDPOINT: "https://s3.bto.bar:8333",
  SEAWEEDFS_S3_ACCESS_KEY_ID: "a",
  SEAWEEDFS_S3_SECRET_ACCESS_KEY: "s",
  GITEA_BASE_URL: "https://gitea.bto.bar",
  GITEA_TOKEN: "t",
};

describe("loadConfig", () => {
  it("applies defaults for bucket, region, and model", () => {
    const cfg = loadConfig(full);
    expect(cfg.s3Bucket).toBe("orrery-agent-sprites");
    expect(cfg.s3Region).toBe("us-east-1");
    expect(cfg.modelId).toBe("gemini-3-pro-image-preview");
  });

  it("lists every missing required var by NAME (never values)", () => {
    try {
      loadConfig({});
      throw new Error("should have thrown");
    } catch (e) {
      const msg = (e as Error).message;
      expect(msg).toContain("GEMINI_API_KEY");
      expect(msg).toContain("SEAWEEDFS_S3_ACCESS_KEY_ID");
      expect(msg).toContain("GITEA_TOKEN");
    }
  });

  it("honors model + bucket overrides", () => {
    const cfg = loadConfig({ ...full, AVATAR_MODEL_ID: "gemini-3.1-flash-image", SEAWEEDFS_S3_BUCKET: "custom" });
    expect(cfg.modelId).toBe("gemini-3.1-flash-image");
    expect(cfg.s3Bucket).toBe("custom");
  });
});
