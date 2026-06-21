export const VERSION = "0.1.0";
export { generateForRepo, type PipelineDeps } from "./pipeline.js";
export { deriveMetadata, parseRepoKey, type GitInspector } from "./metadata/derive.js";
export { Registry } from "./registry.js";
export { RepoMetadataSchema, type RepoMetadata, type FrameEntry } from "./metadata/schema.js";
export { POSE_ORDER, POSE_DESCRIPTIONS, type Pose } from "./poses.js";
export { buildSheetPrompt, SHEET_LAYOUT } from "./prompt.js";
