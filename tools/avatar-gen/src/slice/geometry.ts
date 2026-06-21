export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Inclusive [start, end] index ranges of consecutive `true` runs at least `minRun` long. */
export function detectBands(present: boolean[], minRun: number): Array<[number, number]> {
  const bands: Array<[number, number]> = [];
  let start = -1;
  for (let i = 0; i <= present.length; i++) {
    const on = i < present.length && present[i];
    if (on && start === -1) start = i;
    if (!on && start !== -1) {
      if (i - start >= minRun) bands.push([start, i - 1]);
      start = -1;
    }
  }
  return bands;
}

/**
 * Options for content detection in {@link sliceGrid}.
 */
export interface SliceOptions {
  /**
   * Alpha value (0-255) at or above which a pixel counts as content.
   * Default: 16.
   */
  threshold?: number;
  /**
   * Minimum run length (in pixels) for a content band; shorter runs are treated
   * as noise and dropped. Default: 2 — filters single-pixel anti-aliasing and
   * compression speckle on real generated sheets. Tune via Task 15 validation
   * spike if a real sheet's gutters or cells are mis-detected.
   */
  minRun?: number;
}

/**
 * Detect content cells by alpha projection. First split into horizontal rows
 * (rows with any opaque pixel), then split each row into columns. Returns rects
 * in reading order (top→bottom, left→right).
 */
export function sliceGrid(
  alpha: Uint8Array,
  width: number,
  height: number,
  opts: SliceOptions = {},
): Rect[] {
  const threshold = opts.threshold ?? 16;
  const minRun = opts.minRun ?? 2;
  const opaque = (x: number, y: number): boolean => (alpha[y * width + x] ?? 0) >= threshold;

  const rowPresent: boolean[] = [];
  for (let y = 0; y < height; y++) {
    let any = false;
    for (let x = 0; x < width && !any; x++) any = opaque(x, y);
    rowPresent.push(any);
  }

  const rects: Rect[] = [];
  for (const [y0, y1] of detectBands(rowPresent, minRun)) {
    const colPresent: boolean[] = [];
    for (let x = 0; x < width; x++) {
      let any = false;
      for (let y = y0; y <= y1 && !any; y++) any = opaque(x, y);
      colPresent.push(any);
    }
    for (const [x0, x1] of detectBands(colPresent, minRun)) {
      rects.push({ left: x0, top: y0, width: x1 - x0 + 1, height: y1 - y0 + 1 });
    }
  }
  return rects;
}
