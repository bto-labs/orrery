import { describe, it, expect } from "vitest";
import { parseRepoKey, deriveMetadata } from "../src/metadata/derive.js";
import type { GitInspector } from "../src/metadata/derive.js";
import type { GiteaClient } from "../src/metadata/gitea.js";

const gitea: GiteaClient = {
  async fetchRepo() {
    return {
      description: "GPU ambient visualization",
      topics: ["bevy", "wgpu"],
      primaryLanguage: "Rust",
      createdAt: "2026-06-17T00:00:00Z",
    };
  },
};

const git: GitInspector = {
  async remoteUrl() {
    return "git@gitea.bto.bar:BTO/orrery.git";
  },
  async firstCommitDate() {
    return "2026-06-17T00:00:00Z";
  },
};

describe("parseRepoKey", () => {
  it("parses ssh remotes", () => {
    expect(parseRepoKey("git@gitea.bto.bar:BTO/orrery.git")).toEqual({
      host: "gitea.bto.bar",
      owner: "BTO",
      repo: "orrery",
      repoKey: "gitea.bto.bar/BTO/orrery",
    });
  });
  it("parses https remotes", () => {
    expect(parseRepoKey("https://gitea.bto.bar/BTO/orrery.git")?.repoKey).toBe(
      "gitea.bto.bar/BTO/orrery",
    );
  });
});

describe("deriveMetadata", () => {
  it("derives identity + gitea fields and computes ageDays", async () => {
    const m = await deriveMetadata({ git, gitea, today: "2026-06-20T00:00:00Z" });
    expect(m.repoKey).toBe("gitea.bto.bar/BTO/orrery");
    expect(m.owner).toBe("BTO");
    expect(m.displayName).toBe("orrery");
    expect(m.primaryLanguage).toBe("Rust");
    expect(m.summary).toBe("GPU ambient visualization");
    expect(m.topics).toEqual(["bevy", "wgpu"]);
    expect(m.ageDays).toBe(3);
    expect(m.themeHint).toBeNull(); // curated, not auto-filled
  });

  it("lets curated overrides win", async () => {
    const m = await deriveMetadata({
      git,
      gitea,
      today: "2026-06-20T00:00:00Z",
      curated: { themeHint: "a jewel", displayName: "Orrery" },
    });
    expect(m.themeHint).toBe("a jewel");
    expect(m.displayName).toBe("Orrery");
  });
});
