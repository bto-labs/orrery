import { POSE_ORDER, POSE_DESCRIPTIONS, type Pose } from "./poses.js";
import type { RepoMetadata } from "./metadata/schema.js";

export const SHEET_LAYOUT = {
  rows: 2,
  cols: 3, // 3 cells on the top row, 2 on the bottom (5 total, reading order)
  cells: 5,
  aspectRatio: "4:3",
  imageSize: "2K",
} as const;

function themeLine(meta: RepoMetadata): string {
  const theme = meta.themeHint ?? `a ${meta.primaryLanguage} project named "${meta.displayName}"`;
  const motifs = meta.iconMotifs.length ? ` Incorporate motif(s): ${meta.iconMotifs.join(", ")}.` : "";
  const palette = meta.accentPalette.length
    ? ` Accent colors: ${meta.accentPalette.join(", ")}.`
    : "";
  const personality = meta.personalityTraits.length
    ? ` Personality: ${meta.personalityTraits.join(", ")}.`
    : "";
  return `Theme: ${theme}.${motifs}${palette}${personality}`;
}

function poseLine(pose: Pose, index: number): string {
  return `${index + 1}. "${pose}" — ${POSE_DESCRIPTIONS[pose]}`;
}

export function buildSheetPrompt(meta: RepoMetadata): string {
  const poses = POSE_ORDER.map(poseLine).join("\n");
  return [
    "You are generating a sprite contact sheet for a single friendly robot character.",
    "Use the attached reference image(s) as the exact style anchor for the robot's body, proportions, and rendering style.",
    themeLine(meta),
    "",
    `Produce ONE single image: a contact sheet containing exactly ${SHEET_LAYOUT.cells} poses of THE SAME character.`,
    `Arrange them in reading order across ${SHEET_LAYOUT.rows} rows (${SHEET_LAYOUT.cols} cells on the top row, the remainder on the bottom).`,
    "Hard requirements (a downstream program slices this image):",
    "- Fully transparent background (alpha), no scenery, no shadow plate.",
    "- Wide, fully-transparent gutters separating every pose (both horizontally and vertically).",
    "- Each pose fully contained within its cell; the character is the same robot in every cell, only the pose/expression changes.",
    "- Consistent scale, lighting, and camera across all cells.",
    "The 5 poses, in order:",
    poses,
  ].join("\n");
}
