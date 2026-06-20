import { metadataHash } from "./metadata/hash.js";
import { RepoMetadataSchema, type RepoMetadata } from "./metadata/schema.js";
import { buildSheetPrompt } from "./prompt.js";
import { sliceSheet } from "./slice/index.js";
import { POSE_ORDER } from "./poses.js";
import { frameKey, sheetKey, type FrameStore } from "./storage.js";
import type { ImageGenerator, ReferenceImage } from "./gemini.js";
import type { Registry } from "./registry.js";

export interface PipelineDeps {
  generator: ImageGenerator;
  store: FrameStore;
  registry: Registry;
  baseBots: () => Promise<ReferenceImage[]>;
  now: () => string;
  modelId: string;
}

export async function generateForRepo(
  meta: RepoMetadata,
  deps: PipelineDeps,
  opts: { force?: boolean } = {},
): Promise<RepoMetadata> {
  const hash = metadataHash(meta);

  if (!opts.force && !deps.registry.needsRegeneration(meta)) {
    return deps.registry.get(meta.repoKey) ?? meta;
  }

  const references = await deps.baseBots();
  const prompt = buildSheetPrompt(meta);
  const sheet = await deps.generator.generateSheet({ prompt, references });
  const frames = await sliceSheet(sheet, POSE_ORDER);

  const spriteSheetUri = await deps.store.put(sheetKey(meta.repoKey, hash), sheet, "image/png");
  const frameEntries = [];
  for (const frame of frames) {
    const uri = await deps.store.put(
      frameKey(meta.repoKey, hash, frame.pose),
      frame.png,
      "image/png",
    );
    frameEntries.push({ pose: frame.pose, uri });
  }

  const updated = RepoMetadataSchema.parse({
    ...meta,
    metadataHash: hash,
    generatedAt: deps.now(),
    modelId: deps.modelId,
    spriteSheetUri,
    frames: frameEntries,
  });

  deps.registry.upsert(updated);
  await deps.registry.save();
  return updated;
}
