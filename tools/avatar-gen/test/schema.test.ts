import { describe, it, expect } from "vitest";
import { RepoMetadataSchema } from "../src/metadata/schema.js";

const minimal = {
  repoKey: "gitea.bto.bar/BTO/orrery",
  displayName: "Orrery",
  owner: "BTO",
  isPersonal: false,
  primaryLanguage: "Rust",
};

describe("RepoMetadataSchema", () => {
  it("accepts a minimal object and applies defaults", () => {
    const m = RepoMetadataSchema.parse(minimal);
    expect(m.topics).toEqual([]);
    expect(m.themeHint).toBeNull();
    expect(m.frames).toEqual([]);
    expect(m.metadataHash).toBeNull();
  });

  it("rejects a missing required field", () => {
    const { repoKey: _omit, ...broken } = minimal;
    expect(() => RepoMetadataSchema.parse(broken)).toThrow();
  });

  it("preserves curated fields when present", () => {
    const m = RepoMetadataSchema.parse({ ...minimal, themeHint: "a jewel", iconMotifs: ["jewel"] });
    expect(m.themeHint).toBe("a jewel");
    expect(m.iconMotifs).toEqual(["jewel"]);
  });
});
