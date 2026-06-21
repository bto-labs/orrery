Continuing orrery (GPU ambient visualization of live Claude Code agent activity).
Working directory: /home/jay/dev/orrery, on bto-storm (RTX 5070 Ti).

Where things stand: Stage 1 (live RabbitMQ ingestion) shipped (v0.2.0). Stage 2 is
the pivot to representational bot characters. **Subsystem A — the avatar-generation
pipeline — is complete and merged to `main` locally** (`tools/avatar-gen/`,
avatar-gen v0.1.0; 47 tests green; not pushed). **Subsystem B — the renderer rework
that consumes the generated frames — is not built yet.**

## Read these in order BEFORE doing anything else:

1. docs/handoffs/2026-06-20-stage2-subsystemA-avatar-gen.md
   — the full Subsystem-A handoff: what shipped, the Jay-owned blockers, and the
   prioritized next steps. Load-bearing.
2. docs/superpowers/specs/2026-06-19-stage2-character-avatars-design.md
   — the Stage-2 design (character avatars). §3.1 is the A→B frame-URI contract:
   Subsystem B locates frames via the discrete repoKey+metadataHash+pose fields in
   assets/agents/registry.json, NOT by parsing the opaque `uri`. §7 sketches the
   renderer rework (character sprites, expression-by-state, Rapier/force-field
   motion, orbital hook.* icons, inter-agent arcs, human pairing).
3. CLAUDE.md — the "Stage 2 — avatar-gen (Subsystem A)" section + the new gotchas
   (Gemini model-id churn, @google/genai-not-Rust, secrets-from-env).

## Then, pick the next move:

- If Jay has dropped base bots into assets/bots/base/ and confirmed the Gemini model
  id: run the validation spike (tools/avatar-gen/README.md) to confirm the real
  single-sheet→slice layout before trusting generation.
- Otherwise, the main build step is: **invoke the writing-plans skill to author the
  Subsystem B (renderer rework) implementation plan**, consuming the cached frames
  per the §3.1 contract and landing the deferred Rapier/force-field motion + orbital
  activity icons on top of Stage 1's ingestion seam.

Guardrails: never push without Jay's explicit OK (main has unpushed Stage-2 commits).
Secrets are env-only (OpenBao-injected), never logged. AFFiNE work is parked in the
bto-devops repo (separate session) — nothing to do from orrery.
