import sharp from "sharp";

/**
 * Build a deterministic 2-row (3+2) contact sheet on a transparent background:
 * five opaque colored blocks separated by wide transparent gutters. Mirrors the
 * layout the prompt asks the model to produce, so the slicer can be tested
 * without a live generation.
 */
export async function makeSyntheticSheet(): Promise<Buffer> {
  const W = 600;
  const H = 400;
  const colors = [
    { r: 220, g: 40, b: 40 },
    { r: 40, g: 200, b: 60 },
    { r: 50, g: 80, b: 230 },
    { r: 230, g: 200, b: 30 },
    { r: 180, g: 40, b: 200 },
  ];
  // cell origins (3 on top row, 2 on bottom), each block 120x120 with gutters
  const cells = [
    { x: 40, y: 40 },
    { x: 240, y: 40 },
    { x: 440, y: 40 },
    { x: 40, y: 240 },
    { x: 240, y: 240 },
  ];
  const blocks = cells.map((c, i) => {
    const col = colors[i]!;
    return {
      input: {
        create: { width: 120, height: 120, channels: 4 as const, background: { ...col, alpha: 1 } },
      },
      left: c.x,
      top: c.y,
    };
  });
  return sharp({
    create: { width: W, height: H, channels: 4, background: { r: 0, g: 0, b: 0, alpha: 0 } },
  })
    .composite(blocks)
    .png()
    .toBuffer();
}
