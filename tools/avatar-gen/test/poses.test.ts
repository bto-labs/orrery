import { describe, it, expect } from "vitest";
import { POSE_ORDER, POSE_DESCRIPTIONS } from "../src/poses.js";

describe("pose contract", () => {
  it("has exactly the 5 canonical poses in stable order", () => {
    expect(POSE_ORDER).toEqual(["neutral", "idle", "active", "attention", "error"]);
  });

  it("describes every pose", () => {
    for (const pose of POSE_ORDER) {
      expect(POSE_DESCRIPTIONS[pose]).toBeTruthy();
    }
    expect(Object.keys(POSE_DESCRIPTIONS)).toHaveLength(POSE_ORDER.length);
  });
});
