import { z } from "zod";
import { POSE_ORDER } from "../poses.js";

const PoseEnum = z.enum(POSE_ORDER);

export const FrameEntrySchema = z.object({
  pose: PoseEnum,
  uri: z.string(),
});
export type FrameEntry = z.infer<typeof FrameEntrySchema>;

export const RepoMetadataSchema = z.object({
  // --- identity / auto-derived ---
  repoKey: z.string().min(1),
  displayName: z.string().min(1),
  owner: z.string().min(1),
  isPersonal: z.boolean(),
  primaryLanguage: z.string().min(1),
  createdAt: z.string().nullable().default(null),
  ageDays: z.number().int().nonnegative().nullable().default(null),
  summary: z.string().default(""),
  category: z
    .enum(["infra", "app", "library", "bot", "visualization", "data", "docs", "unknown"])
    .default("unknown"),
  topics: z.array(z.string()).default([]),
  hosts: z.array(z.string()).default([]),

  // --- curated overrides (start blank; Jay edits) ---
  themeHint: z.string().nullable().default(null),
  goals: z.string().nullable().default(null),
  personalityTraits: z.array(z.string()).default([]),
  accentPalette: z.array(z.string()).default([]),
  baseBotPreference: z.string().nullable().default(null),
  iconMotifs: z.array(z.string()).default([]),

  // --- system / generation bookkeeping ---
  metadataHash: z.string().nullable().default(null),
  generatedAt: z.string().nullable().default(null),
  modelId: z.string().nullable().default(null),
  spriteSheetUri: z.string().nullable().default(null),
  frames: z.array(FrameEntrySchema).default([]),
});

export type RepoMetadata = z.infer<typeof RepoMetadataSchema>;
