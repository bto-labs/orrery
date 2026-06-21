import sharp from "sharp";
import { sliceGrid } from "./geometry.js";
import type { Pose } from "../poses.js";

function alphaChannel(data: Buffer, width: number, height: number, channels: number): Uint8Array {
  const alpha = new Uint8Array(width * height);
  const aIndex = channels - 1; // RGBA → alpha is the last channel
  for (let i = 0; i < width * height; i++) {
    alpha[i] = channels >= 4 ? (data[i * channels + aIndex] ?? 255) : 255;
  }
  return alpha;
}

export async function sliceSheet(
  sheet: Buffer,
  poses: readonly Pose[],
): Promise<Array<{ pose: Pose; png: Buffer }>> {
  const { data, info } = await sharp(sheet).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
  const alpha = alphaChannel(data, info.width, info.height, info.channels);
  const rects = sliceGrid(alpha, info.width, info.height);

  if (rects.length !== poses.length) {
    throw new Error(`slice: expected ${poses.length} cells, detected ${rects.length}`);
  }

  const out: Array<{ pose: Pose; png: Buffer }> = [];
  for (let i = 0; i < rects.length; i++) {
    const png = await sharp(sheet).extract(rects[i]!).png().toBuffer();
    out.push({ pose: poses[i]!, png });
  }
  return out;
}
