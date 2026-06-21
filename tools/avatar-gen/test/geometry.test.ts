import { describe, it, expect } from "vitest";
import { detectBands, sliceGrid } from "../src/slice/geometry.js";

describe("detectBands", () => {
  it("finds runs of content separated by gutters", () => {
    // present = content. two blocks: [1..3] and [6..8]
    const present = [false, true, true, true, false, false, true, true, true, false];
    expect(detectBands(present, 1)).toEqual([
      [1, 3],
      [6, 8],
    ]);
  });

  it("ignores runs shorter than minRun (noise)", () => {
    const present = [true, false, false, false, false, true, true, true, true];
    expect(detectBands(present, 2)).toEqual([[5, 8]]);
  });
});

describe("sliceGrid", () => {
  it("splits a 2-row / 3-then-2 layout into 5 reading-order rects", () => {
    // Build a 14x9 alpha grid: rows 1..3 content, row 4 gutter, rows 5..7 content.
    // Top row content cols: 1..2, 5..6, 9..10  (3 cells)
    // Bottom row content cols: 1..2, 5..6       (2 cells)
    const w = 14;
    const h = 9;
    const alpha = new Uint8Array(w * h);
    const set = (x0: number, x1: number, y0: number, y1: number) => {
      for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) alpha[y * w + x] = 255;
    };
    set(1, 2, 1, 3);
    set(5, 6, 1, 3);
    set(9, 10, 1, 3);
    set(1, 2, 5, 7);
    set(5, 6, 5, 7);

    const rects = sliceGrid(alpha, w, h, { threshold: 128, minRun: 1 });
    expect(rects).toHaveLength(5);
    // reading order: first three from the top row, then two from the bottom
    expect(rects[0]).toEqual({ left: 1, top: 1, width: 2, height: 3 });
    expect(rects[1]).toEqual({ left: 5, top: 1, width: 2, height: 3 });
    expect(rects[2]).toEqual({ left: 9, top: 1, width: 2, height: 3 });
    expect(rects[3]).toEqual({ left: 1, top: 5, width: 2, height: 3 });
    expect(rects[4]).toEqual({ left: 5, top: 5, width: 2, height: 3 });
  });
});
