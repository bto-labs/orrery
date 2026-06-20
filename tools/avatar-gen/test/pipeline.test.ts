import { describe, it, expect, beforeEach } from "vitest";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Registry } from "../src/registry.js";
import { RepoMetadataSchema } from "../src/metadata/schema.js";
import { metadataHash } from "../src/metadata/hash.js";
import { POSE_ORDER } from "../src/poses.js";
import { makeSyntheticSheet } from "../src/__fixtures__/make-sheet.js";
import { generateForRepo, type PipelineDeps } from "../src/pipeline.js";
import type { ImageGenerator } from "../src/gemini.js";
import type { FrameStore } from "../src/storage.js";

const meta = RepoMetadataSchema.parse({
  repoKey: "gitea.bto.bar/BTO/orrery",
  displayName: "Orrery",
  owner: "BTO",
  isPersonal: false,
  primaryLanguage: "Rust",
  themeHint: "a jewel",
});

function fakeStore(): FrameStore & { puts: string[] } {
  const puts: string[] = [];
  return {
    puts,
    async put(key) {
      puts.push(key);
      return `s3://orrery-agent-sprites/${key}`;
    },
    async exists() {
      return false;
    },
  };
}

let regPath: string;
beforeEach(() => {
  const dir = mkdtempSync(join(tmpdir(), "pipe-"));
  regPath = join(dir, "registry.json");
  writeFileSync(regPath, JSON.stringify({ version: 1, repos: {} }));
});

async function deps(genCount: { n: number }, store: FrameStore): Promise<PipelineDeps> {
  const sheet = await makeSyntheticSheet();
  const generator: ImageGenerator = {
    async generateSheet() {
      genCount.n += 1;
      return sheet;
    },
  };
  return {
    generator,
    store,
    registry: await Registry.load(regPath),
    baseBots: async () => [{ data: Buffer.from("REF"), mimeType: "image/png" }],
    now: () => "2026-06-20T00:00:00Z",
    modelId: "gemini-3-pro-image-preview",
  };
}

describe("generateForRepo", () => {
  it("generates, slices, uploads sheet + 5 frames, and records the registry", async () => {
    const genCount = { n: 0 };
    const store = fakeStore();
    const d = await deps(genCount, store);
    const out = await generateForRepo(meta, d);

    expect(genCount.n).toBe(1);
    expect(out.frames.map((f) => f.pose)).toEqual([...POSE_ORDER]);
    expect(out.metadataHash).toBe(metadataHash(meta));
    expect(out.modelId).toBe("gemini-3-pro-image-preview");
    expect(store.puts).toHaveLength(6); // 1 sheet + 5 frames
    expect(store.puts).toContain("gitea.bto.bar/BTO/orrery/" + metadataHash(meta) + "/idle.png");

    const persisted = await Registry.load(regPath);
    expect(persisted.get(meta.repoKey)?.frames).toHaveLength(5);
  });

  it("short-circuits on a cache hit (no generation)", async () => {
    // pre-seed a complete generation
    const seed = await Registry.load(regPath);
    seed.upsert(
      RepoMetadataSchema.parse({
        ...meta,
        metadataHash: metadataHash(meta),
        frames: POSE_ORDER.map((pose) => ({ pose, uri: `s3://b/${pose}.png` })),
      }),
    );
    await seed.save();

    const genCount = { n: 0 };
    const store = fakeStore();
    const d = await deps(genCount, store);
    await generateForRepo(meta, d);
    expect(genCount.n).toBe(0);
    expect(store.puts).toHaveLength(0);
  });

  it("regenerates a cache hit when force is set", async () => {
    const seed = await Registry.load(regPath);
    seed.upsert(
      RepoMetadataSchema.parse({
        ...meta,
        metadataHash: metadataHash(meta),
        frames: POSE_ORDER.map((pose) => ({ pose, uri: `s3://b/${pose}.png` })),
      }),
    );
    await seed.save();

    const genCount = { n: 0 };
    const store = fakeStore();
    const d = await deps(genCount, store);
    await generateForRepo(meta, d, { force: true });
    expect(genCount.n).toBe(1);
  });
});
