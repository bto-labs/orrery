# Stage 2 — Character Avatars & AI-Themed Agent Identities (design spec)

- **Status:** approved design, pre-implementation
- **Date:** 2026-06-19
- **Builds on:** Stage 1 (live ingestion, v0.2.0). **Revises the visual direction**
  of `PLAN.md` (see §1).
- **Next step:** implementation plans via writing-plans — note this spec
  decomposes into independent plans (§9).

## 1. What changes, and why

The original `PLAN.md` borrowed Observatory's **abstract** visual metaphor: agents
as glowing *nuclei*, tools/files as abstract *orbital bodies*, idle=breathing /
active=pulsing. Stages 0–1 shipped the nucleus (a glowing, model-colored dot) +
the data pipeline. In review, that reads as "just colored dots" — far from the
goal.

**Stage 2 makes the visualization representational, not abstract.** Each agent is
a **bot character**, not a glow; the spec's "orbital bodies" become **activity
icons orbiting the character** (wrench/gear/speech-bubble/"!" driven by live
`hook.*` events); a human-in-the-loop Claude Code session renders as a **human +
bot pair**. Identity is keyed to the workspace: the **human is always the same
(Jay)**; every other bot's **appearance is themed per project/repo**, so two
repos never look alike.

This keeps the *structure* the spec intended (a character "nucleus" surrounded by
orbital activity bodies + inter-agent arcs) but swaps the abstract aesthetic for
legible characters — a better fit for "an ambient view of *who* is working on
*what*."

## 2. The avatar pipeline (core idea)

```
 base bots (Jay delivers, 3–5)  ┐
 project metadata (per repo)    ┼─▶ Gemini image model ─▶ slice ─▶ cache ─▶ Rust renderer
                                ┘   (themed bot, 5 poses,   (5 frames) (SeaweedFS)  (loads sprites)
                                     one consistency sheet)
```

- **Base bots** (3–5, human-delivered) are the **style anchor** — every generation
  references them so the fleet shares a coherent look. Committed to the repo at
  `assets/bots/base/`.
- **Per-repo metadata** (§4) themes each agent (e.g. bto-coder → a coding bot with
  a keyboard motif; orrery → a jewel motif).
- The **image model** (Gemini "Nano Banana" family) generates a **themed bot in 5
  poses as a single contact sheet** (§5 — the consistency trick), which is sliced
  into 5 frames.
- Frames are **cached in SeaweedFS** keyed by `repoKey + metadataHash`; the
  renderer loads them. Generation is **async + one-time per repo**, never on the
  render path.

## 3. Decomposition (two independent subsystems, clean interface)

The interface between them is **"5 cached sprite frames per repo + their pose
labels."** They can be built and shipped separately.

- **Subsystem A — avatar-generation pipeline** (a worker; not Rust): metadata
  registry → Gemini generation → slice → cache to SeaweedFS. §4–§6.
- **Subsystem B — renderer rework** (orrery, Rust/Bevy): consume cached frames;
  characters with expression-by-state; physics/force-field motion; orbital
  activity icons; inter-agent arcs; the human pairing. §7.

## 4. Project metadata schema

Auto-derived as a **placeholder**; Jay fills the creative fields. Stored in a
registry (`assets/agents/registry.json`, or a small service) keyed by `repoKey`.

```jsonc
{
  // --- identity / auto-derived (git + gitea + workspace) ---
  "repoKey": "gitea.bto.bar/BTO/orrery",   // canonical id (also the cache key root)
  "displayName": "Orrery",
  "owner": "BTO", "isPersonal": false,
  "primaryLanguage": "Rust",
  "createdAt": "2026-06-17", "ageDays": 2,
  "summary": "GPU ambient visualization of live Claude Code activity",  // gitea desc / README
  "category": "visualization",             // heuristic: infra|app|library|bot|visualization|data|docs
  "topics": ["bevy", "wgpu", "rabbitmq"],
  "hosts": ["bto-storm"],

  // --- curated overrides (start blank; Jay edits — the creative layer) ---
  "themeHint": "a jewel / an orrery (clockwork solar-system model)",   // THE motif
  "goals": "ambient awareness of the whole homelab agent fleet",
  "personalityTraits": ["calm", "watchful", "ornamental"],
  "accentPalette": ["#6a5acd", "#19c3c9"],
  "baseBotPreference": null,               // optional: pin one base bot to anchor on
  "iconMotifs": ["jewel", "orbit"],

  // --- system / generation bookkeeping ---
  "metadataHash": "…",                     // hash of the above; cache invalidation key
  "generatedAt": null, "modelId": null,
  "spriteSheetUri": null,                  // SeaweedFS object
  "frames": []                             // [{pose:"idle", uri:"…"}, …] after slicing
}
```

Auto-derivation sources: workspace `cwd`/git remote → `repoKey`/`owner`/
`isPersonal`; gitea API → description, topics, language, created date; git log →
age/last-active. New repos get auto defaults + a generic base bot until themed.

## 5. Generation worker (Subsystem A)

- **Runtime:** a small **TypeScript worker** using the official **`@google/genai`
  SDK** — best-supported path for image output, reference images, and the
  single-sheet technique. (Deliberately **not** the Rust renderer, and **not**
  gated on an immature Rust Gemini crate. Google's OpenAI-compat endpoint has thin
  image-gen support; the native `generateContent` API is the reliable path.)
- **Auth:** Gemini Tier-1 key from **OpenBao** (per `service-secrets-via-openbao`).
- **Model:** the latest Gemini image model (the "Nano Banana" family, e.g.
  `gemini-2.5-flash-image`). **Confirm the current model id at build** against the
  Gemini model-list (names churn). Optionally route via **litellm** later.
- **The consistency trick (load-bearing — naive generation fails here):** image
  models are bad at "same character across 5 *separate* generations." So:
  1. Pass the chosen **base bot(s) as reference image(s)** (style anchor).
  2. Ask for **ONE image** — a labeled **contact sheet of all 5 poses**, uniform/
     transparent background, evenly spaced, *the same bot* — so consistency is
     guaranteed *within one generation*.
  3. **Slice** the sheet into 5 frames (fixed grid, known layout).
- **Caching / regeneration:** key = `repoKey + metadataHash`. Store sliced frames
  in **SeaweedFS** (same store as the AFFiNE artifact infra). Metadata change →
  new hash → regenerate. Renderer cache-miss → show a base-bot placeholder + fire
  generation async.
- **Cost/latency:** one-time per repo, cached forever, off the render path —
  negligible at homelab scale.

## 6. The 5 canonical poses (generation ↔ state contract)

The 5 poses are a fixed contract so generation output maps to render state:

| Pose | Drives (AgentState) |
|---|---|
| `idle` (pensive/resting) | status Idle |
| `active` (working / raising hand) | status Active |
| `attention` (looking aside / alert) | Notification / awaiting input |
| `error` (concerned) | status Error |
| `neutral` (default/transition) | spawn / fallback |

## 7. Renderer rework (Subsystem B — orrery, Rust/Bevy)

- **Character sprites:** replace the glow-blob `Nucleus` render with the cached
  character frames (Bevy 2D sprite/atlas). Keyed by `session_id`; appearance by
  `repoKey` (workspace).
- **Expression-by-state:** pick the pose frame matching status (§6), cross-dissolve
  on change. (See §8A — this is the realistic interpretation of "animate the
  deltas.")
- **Procedural / physics motion:** the character floats/bobs/leans via the
  force-field; this is where **Rapier 2.5D** (deferred from Stage 1) lands —
  attraction/tether of orbital bodies, inter-agent repulsion, soft wander.
- **Orbital activity icons** = the spec's "orbital bodies," made representational:
  a live `hook.*` event spawns an icon that orbits the character — ToolUse →
  wrench/gear, UserPrompt/AssistantMessage → speech bubble, Attention → "!".
  Icons decay/despawn. This is the direct use of the events Stage 1 currently
  throws away as a one-frame flare.
- **Inter-agent arcs:** bezier arcs + flowing particles between related sessions
  (subagents via `parentUuid`/`agent_id` from the transcript/`rawPayload`).
- **The human (Jay):** one curated, never-generated avatar. Sessions that are
  human-in-the-loop render as **human + bot**; autonomous agents render bot-only.
- **Interactive vs autonomous classification (heuristic, refine later):** a session
  is interactive (→ human+bot) if it emits `UserPromptSubmit` hook events; else
  bot-only. Allow an override list of known-autonomous agents (slack-bot,
  systems-engineer).

## 8. Honest caveats (designed-around, not into)

**A. "Animate the deltas between the 5 poses" — you can't truly tween raster poses
without a rig.** v1 = **expression-by-state** (pose selected by status, cross-
dissolve on change) **+ procedural/physics secondary motion** (float/bob/lean) **+
the orbital activity icons**. That already reads as alive. **True pose morphing**
(optical-flow morph, or 2D rigging à la Live2D/Spine on the generated frames) is a
real fidelity upgrade — explicit **later phase**, not v1.

**B. Character consistency** across poses is the classic image-gen failure — solved
by the single-sheet + reference-image technique (§5), and Gemini's "Nano Banana"
family is specifically strong at character consistency + reference editing.

**C. Aesthetic at scale:** many detailed character scenes can get busy vs. the calm
ambient ideal. Keep the cap (`max_agents`) and the performance ladder; consider an
abstract/“far” fallback when the field is dense (future tuning, not v1 blocker).

## 9. Implementation plans this spec yields (for writing-plans)

1. **Avatar-generation pipeline** (Subsystem A): metadata registry + auto-derivation;
   the TS generation worker (Gemini via OpenBao key, single-sheet + slice); SeaweedFS
   caching + regeneration. Ships independently (output = cached frames).
2. **Renderer rework** (Subsystem B): character sprites + expression-by-state;
   Rapier/force-field motion; orbital activity icons from `hook.*`; inter-agent arcs;
   human pairing + interactive/autonomous classification.
3. **Later — pose morphing / 2D rig fidelity** (Subsystem B, phase 2).

## 10. Out of scope / deferred

True pose-morphing/2D-rigging (later phase); idle/D-Bus screensaver trigger
(Stage 4); dashboard text panels; the abstract-at-scale fallback; multi-monitor
spanning. Mimir/REST stay dropped (§9 Stage-1 verification).

## 11. Open items

- Jay delivers the 3–5 base bots → `assets/bots/base/`.
- Confirm the current Gemini image model id at build.
- Finalize the metadata auto-derivation sources (gitea API fields) + the registry
  location (in-repo JSON vs a small service).
- Validate the single-sheet→slice layout with a real generation before committing
  the prompt template.
- Refine the interactive-vs-autonomous heuristic against real fleet traffic.

## 12. References

- Visual direction: Jay's reference images (single bot; bot + orbital activity
  icons; human+bot collaboration) — 2026-06-19.
- Original metaphor: `PLAN.md` ("nucleus + orbital bodies + arcs"); this spec makes
  it representational.
- Infra synergy: the SeaweedFS artifact store from the AFFiNE design
  (`bto-devops/docs/superpowers/specs/2026-06-19-affine-visual-workspace-design.md`).
