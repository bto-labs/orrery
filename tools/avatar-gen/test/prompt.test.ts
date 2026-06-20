import { describe, it, expect } from "vitest";
import { buildSheetPrompt, SHEET_LAYOUT } from "../src/prompt.js";
import { POSE_ORDER } from "../src/poses.js";
import { RepoMetadataSchema } from "../src/metadata/schema.js";

const meta = RepoMetadataSchema.parse({
  repoKey: "gitea.bto.bar/BTO/orrery",
  displayName: "Orrery",
  owner: "BTO",
  isPersonal: false,
  primaryLanguage: "Rust",
  themeHint: "a jewel / clockwork orrery",
  iconMotifs: ["jewel", "orbit"],
  accentPalette: ["#6a5acd"],
});

describe("buildSheetPrompt", () => {
  it("names all 5 poses in reading order", () => {
    const p = buildSheetPrompt(meta);
    for (const pose of POSE_ORDER) expect(p).toContain(pose);
    expect(p.indexOf("neutral")).toBeLessThan(p.indexOf("error"));
  });

  it("encodes the theme and the slicing-critical layout constraints", () => {
    const p = buildSheetPrompt(meta);
    expect(p).toContain("a jewel / clockwork orrery");
    expect(p.toLowerCase()).toContain("transparent background");
    expect(p.toLowerCase()).toContain("single image");
    expect(p.toLowerCase()).toContain("same"); // same character across cells
    expect(p).toContain(`${SHEET_LAYOUT.cols}`);
  });

  it("falls back gracefully when curated theme is blank", () => {
    const blank = RepoMetadataSchema.parse({
      repoKey: "x/y/z",
      displayName: "Z",
      owner: "y",
      isPersonal: false,
      primaryLanguage: "Go",
    });
    const p = buildSheetPrompt(blank);
    expect(p).toContain("Go"); // language used as a theme hint fallback
    expect(p.length).toBeGreaterThan(100);
  });
});
