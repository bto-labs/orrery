import { describe, it, expect } from "vitest";
import { RepoMetadataSchema } from "../src/metadata/schema.js";
import { metadataHash } from "../src/metadata/hash.js";

const base = RepoMetadataSchema.parse({
  repoKey: "gitea.bto.bar/BTO/orrery",
  displayName: "Orrery",
  owner: "BTO",
  isPersonal: false,
  primaryLanguage: "Rust",
  topics: ["bevy", "wgpu"],
  themeHint: "a jewel",
});

describe("metadataHash", () => {
  it("is stable for identical logical input", () => {
    const a = metadataHash(base);
    const b = metadataHash(RepoMetadataSchema.parse({ ...base }));
    expect(a).toBe(b);
    expect(a).toMatch(/^[0-9a-f]{16}$/);
  });

  it("is independent of array order", () => {
    const reordered = RepoMetadataSchema.parse({ ...base, topics: ["wgpu", "bevy"] });
    expect(metadataHash(reordered)).toBe(metadataHash(base));
  });

  it("changes when a curated field changes", () => {
    const themed = RepoMetadataSchema.parse({ ...base, themeHint: "a clockwork orrery" });
    expect(metadataHash(themed)).not.toBe(metadataHash(base));
  });

  it("ignores volatile + system fields (no spurious regeneration)", () => {
    const aged = RepoMetadataSchema.parse({
      ...base,
      ageDays: 999,
      createdAt: "2020-01-01",
      hosts: ["bto-storm"],
      generatedAt: "2026-06-20T00:00:00Z",
      modelId: "gemini-3-pro-image-preview",
      spriteSheetUri: "s3://x/y",
      frames: [{ pose: "idle", uri: "s3://x/idle.png" }],
    });
    expect(metadataHash(aged)).toBe(metadataHash(base));
  });
});
