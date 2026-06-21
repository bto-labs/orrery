export const POSE_ORDER = ["neutral", "idle", "active", "attention", "error"] as const;

export type Pose = (typeof POSE_ORDER)[number];

/** Generation-side description of each pose. Maps 1:1 to renderer AgentState (spec §6). */
export const POSE_DESCRIPTIONS: Record<Pose, string> = {
  neutral: "standing calmly, facing forward, relaxed neutral expression (default / transition)",
  idle: "pensive and resting, looking slightly downward, contemplative (idle)",
  active: "energetically working, one hand raised mid-gesture, focused and busy (active)",
  attention: "alert and looking to one side, eyebrows raised as if called (needs attention)",
  error: "concerned, slight frown, shoulders lowered (error state)",
};
