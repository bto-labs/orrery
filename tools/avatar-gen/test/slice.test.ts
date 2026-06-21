import { describe, it, expect } from "vitest";
import sharp from "sharp";
import { makeSyntheticSheet } from "../src/__fixtures__/make-sheet.js";
import { sliceSheet } from "../src/slice/index.js";
import { POSE_ORDER } from "../src/poses.js";

describe("sliceSheet", () => {
  it("splits a synthetic sheet into 5 pose-labeled frames", async () => {
    const sheet = await makeSyntheticSheet();
    const frames = await sliceSheet(sheet, POSE_ORDER);
    expect(frames.map((f) => f.pose)).toEqual([...POSE_ORDER]);
    // each frame should be ~120x120 (the block size), not the whole sheet
    const meta0 = await sharp(frames[0]!.png).metadata();
    expect(meta0.width).toBeGreaterThanOrEqual(110);
    expect(meta0.width).toBeLessThanOrEqual(130);
  });

  it("throws when the cell count does not match the pose count", async () => {
    const sheet = await makeSyntheticSheet();
    await expect(sliceSheet(sheet, ["neutral", "idle"])).rejects.toThrow(/expected 2/);
  });
});
