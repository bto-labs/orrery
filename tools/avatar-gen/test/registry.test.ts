import { describe, it, expect, beforeEach } from "vitest";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Registry } from "../src/registry.js";
import { RepoMetadataSchema } from "../src/metadata/schema.js";
import { metadataHash } from "../src/metadata/hash.js";
import { POSE_ORDER } from "../src/poses.js";

const meta = RepoMetadataSchema.parse({
  repoKey: "gitea.bto.bar/BTO/orrery",
  displayName: "Orrery",
  owner: "BTO",
  isPersonal: false,
  primaryLanguage: "Rust",
  themeHint: "a jewel",
});

let path: string;
beforeEach(() => {
  const dir = mkdtempSync(join(tmpdir(), "reg-"));
  path = join(dir, "registry.json");
  writeFileSync(path, JSON.stringify({ version: 1, repos: {} }));
});

describe("Registry", () => {
  it("upserts and round-trips through disk", async () => {
    const reg = await Registry.load(path);
    reg.upsert(meta);
    await reg.save();
    const reloaded = await Registry.load(path);
    expect(reloaded.get(meta.repoKey)?.displayName).toBe("Orrery");
  });

  it("needsRegeneration is true for an unknown repo", async () => {
    const reg = await Registry.load(path);
    expect(reg.needsRegeneration(meta)).toBe(true);
  });

  it("needsRegeneration is false when hash matches and all 5 frames exist", async () => {
    const reg = await Registry.load(path);
    const done = RepoMetadataSchema.parse({
      ...meta,
      metadataHash: metadataHash(meta),
      frames: POSE_ORDER.map((pose) => ({ pose, uri: `s3://b/${pose}.png` })),
    });
    reg.upsert(done);
    expect(reg.needsRegeneration(meta)).toBe(false);
  });

  it("needsRegeneration is true when the hash drifts", async () => {
    const reg = await Registry.load(path);
    const done = RepoMetadataSchema.parse({
      ...meta,
      metadataHash: "deadbeefdeadbeef",
      frames: POSE_ORDER.map((pose) => ({ pose, uri: `s3://b/${pose}.png` })),
    });
    reg.upsert(done);
    expect(reg.needsRegeneration(meta)).toBe(true);
  });
});
