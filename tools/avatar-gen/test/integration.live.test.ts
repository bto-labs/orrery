import { describe, it, expect } from "vitest";
import { writeFile, mkdir } from "node:fs/promises";
import { join } from "node:path";
import { GoogleGenAI } from "@google/genai";
import { buildSheetPrompt, SHEET_LAYOUT } from "../src/prompt.js";
import { GeminiImageGenerator } from "../src/gemini.js";
import { sliceSheet } from "../src/slice/index.js";
import { POSE_ORDER } from "../src/poses.js";
import { RepoMetadataSchema } from "../src/metadata/schema.js";
import { readFile, readdir } from "node:fs/promises";
import { extname } from "node:path";

const RUN = process.env.GEMINI_API_KEY ? describe : describe.skip;

RUN("live Gemini generation + slice (gated on GEMINI_API_KEY)", () => {
  it("produces a sheet that slices into exactly 5 frames", async () => {
    const meta = RepoMetadataSchema.parse({
      repoKey: "gitea.bto.bar/BTO/orrery",
      displayName: "Orrery",
      owner: "BTO",
      isPersonal: false,
      primaryLanguage: "Rust",
      themeHint: "a jewel / clockwork orrery",
      iconMotifs: ["jewel", "orbit"],
    });

    const dir = process.env.AVATAR_BASE_BOTS ?? "../../assets/bots/base";
    const botFiles = (await readdir(dir)).filter((f) => extname(f).toLowerCase() === ".png").sort();
    const references = botFiles.length
      ? [{ data: await readFile(join(dir, botFiles[0]!)), mimeType: "image/png" }]
      : [];

    const ai = new GoogleGenAI({ apiKey: process.env.GEMINI_API_KEY });
    const generator = new GeminiImageGenerator(ai, process.env.AVATAR_MODEL_ID ?? "gemini-3-pro-image-preview", {
      aspectRatio: SHEET_LAYOUT.aspectRatio,
      imageSize: SHEET_LAYOUT.imageSize,
    });

    const sheet = await generator.generateSheet({ prompt: buildSheetPrompt(meta), references });

    const out = join(process.cwd(), "spike-out");
    await mkdir(out, { recursive: true });
    await writeFile(join(out, "_sheet.png"), sheet);

    const frames = await sliceSheet(sheet, POSE_ORDER);
    for (const f of frames) await writeFile(join(out, `${f.pose}.png`), f.png);

    expect(frames).toHaveLength(POSE_ORDER.length);
  }, 120_000);
});
