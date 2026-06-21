# Changelog

All notable changes to orrery are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); the `[orrery]` entries' versions
track `Cargo.toml` (the Rust renderer crate). Subsystems that ship as their own
package (e.g. `tools/avatar-gen/`) are versioned independently under their own
heading.

## [avatar-gen] v0.1.0 — 2026-06-20

Stage 2 **Subsystem A** — the avatar-generation pipeline. A standalone Node.js +
TypeScript worker (`tools/avatar-gen/`, decoupled from the Rust crate) that turns
per-repo metadata + base bots into 5 cached, themed character-sprite frames (one
per canonical pose) in SeaweedFS, keyed for regeneration on metadata change. This
realizes the Stage-2 pivot (`docs/superpowers/specs/2026-06-19-stage2-character-avatars-design.md`):
agents become representational **bot characters** themed per repo, not abstract
glowing dots. The Rust renderer (Subsystem B) that consumes these frames is not
built yet; this ships the generation half. **No renderer/app behaviour changed**
(`Cargo.toml` stays v0.2.0).

### Added
- `tools/avatar-gen/` — Node 20+/TS worker (npm), built test-first (47 unit tests;
  `tsc --noEmit` clean over both `src` and `test`). Every external boundary
  (Gemini, S3, gitea, git, clock) sits behind an injected interface, so the pure
  logic is unit-tested with fakes; the live model call is a gated spike.
- Metadata: `RepoMetadata` zod schema (spec §4); auto-derivation from git remote +
  gitea API with curated-override merge; `metadataHash` cache-invalidation key over
  identity + curated fields only (volatile/system fields excluded; `accentPalette`
  order significant, other arrays normalized).
- Registry: `assets/agents/registry.json` load/save (keyed by `repoKey`), with a
  `needsRegeneration` decision (hash match + all 5 poses present) that degrades to
  an empty registry on a missing/invalid file instead of throwing.
- Generation: the 5 canonical poses (`neutral, idle, active, attention, error`) as
  a fixed contract; a single contact-sheet prompt (the consistency trick — base
  bots as reference images, one image, 5 poses); a `@google/genai` adapter
  (`gemini-3-pro-image-preview` default, `AVATAR_MODEL_ID` override).
- Slicing: pure alpha-projection geometry → `sharp`-based slicer that splits the
  sheet into 5 pose-labeled frames (throws on a cell-count mismatch).
- Storage: SeaweedFS S3 frame store (path-style), keys `repoKey/metadataHash/<pose>.png`;
  `exists()` discriminates 404 from real infra errors.
- Pipeline orchestrator (`generateForRepo`): derive → hash → cache check → generate
  → slice → upload (sheet + 5 frames) → registry; reads no wall clock (injected
  `now()`); cache-hit short-circuits with a loud invariant on inconsistency.
- CLI (`derive` / `generate`, `--dry-run`, `--force`): `derive` and `--dry-run`
  require only Gitea config; full Gemini/S3 secrets are required only on the real
  `generate`. Secrets come only from env; config errors list var names, never values.
- A gated live spike (`test/integration.live.test.ts`, run only with `GEMINI_API_KEY`)
  + runbook — the de-risking gate validating model/auth/prompt/slice on a real sheet
  before the layout is trusted.
- `assets/bots/base/` (style-anchor bots, human-delivered) + `assets/agents/registry.json`.

### Added (docs)
- `docs/superpowers/plans/2026-06-20-stage2-subsystemA-avatar-generation-pipeline.md`
  (the implementation plan).
- `docs/superpowers/specs/2026-06-19-stage2-character-avatars-design.md` §3.1 — the
  realized frame-URI contract (Subsystem A → B): locate frames via the discrete
  `repoKey` + `metadataHash` + `pose` fields, not by parsing the opaque `uri`.

## [orrery] v0.2.0 — 2026-06-19

Stage 1: real data ingestion. orrery now visualizes live Claude Code sessions
across the homelab, ingested from the `claude-events` RabbitMQ exchange, through
a tokio runtime → reducer → the existing lock-free `triple_buffer` seam. The
synthetic generator is retained as `--synthetic`.

### Added
- `src/ingest/` module: a dedicated tokio runtime (own OS thread) feeding a single
  reducer that owns the merged per-session model (`HashMap<SessionId, AgentState>`)
  and is the only writer to `triple_buffer`. Sources normalize input into a shared
  `AgentUpdate` enum over a bounded mpsc; the reducer reads no wall clock
  (deterministic, unit-tested lifecycle: spawn-on-start, idle-after-timeout,
  despawn-after-TTL).
- Live RabbitMQ hook source (`lapin` 4.10): consumes `hook.#`, maps the
  `HookRelayMessage` envelope to session lifecycle / activity / attention updates;
  reconnect-with-capped-backoff supervisor, no panics on network paths.
- Transcript model source: consumes `transcript.message`, learns each session's
  model once, emits `Summary{model}` so live nuclei colour by model family.
- Dynamic nuclei: the render side reconciles one nucleus per `session_id` with
  fade-in/out and a deterministic per-session screen layout (replacing the fixed
  16-nucleus Stage-0 set).
- Config: live-by-default with `--synthetic` / `--no-rabbitmq` / `--no-transcript`
  toggles, `--idle-ms` / `--despawn-ms` / `--max-agents`, and `RABBITMQ_URL` /
  `CLAUDE_EVENTS_EXCHANGE` env. Bounded AMQP queues (`x-message-ttl` +
  `x-max-length`). 26 unit tests incl. fixture-based source parsers.
- Stage 1 design spec + foundation plan, Plan 2 (live RabbitMQ) design + plan, and
  the §9 source-availability verification, all under `docs/superpowers/`.

### Changed
- Render path flipped from the synthetic `std::thread` producer onto the ingest
  seam; `AgentState` is keyed by `session_id` (String) with a `model: String` +
  `hue_for_model()` mapping (replacing the fixed `Model` enum).
- Default run mode is now live (was synthetic).

### Removed
- `src/agent.rs` and `src/sync.rs` (superseded by `src/ingest/*`).
- The committed `docs/poc-screenshot.png` (5.5 MB binary).
- Mimir and REST ingestion paths — dropped after the §9 verification found no
  live `claude_code_*` metrics and no live REST session feed.

## [orrery] v0.1.0 — 2026-06-17

Stage-0 proof of concept: a Bevy/wgpu binary that de-risks the render stack and
the core architecture for the GPU-accelerated ambient agent visualization, plus
the approved design + plan for Stage 1 (real data ingestion).

### Added
- Bevy 0.18.1 binary (`src/{main,agent,sync,visuals,diagnostics}.rs`) rendering
  soft glowing nuclei + an ambient mote field with HDR + bloom at native 5K.
- Lock-free `triple_buffer` seam between a synthetic `std::thread` producer and
  the Bevy render loop, with framerate-independent exponential smoothing
  (`displayed += (target − displayed)·(1 − e^(−dt/τ))`) and discrete pulse flares
  via a monotonic counter that survives the lossy latest-only read.
- Borderless-fullscreen Wayland window on the current monitor (never exclusive),
  HDR camera + runtime-toggleable bloom, on-screen diagnostics overlay, and
  startup logging of backend / adapter / driver / true physical resolution.
- Runtime controls (Esc quit, B bloom toggle, +/- mote count) and config via
  env/flags (`--agents`, `--motes`, `--no-bloom`, `--no-vsync`, `--screenshot`,
  `ORRERY_*`). 8 unit tests covering the random walk, pulse monotonicity,
  determinism, and easing convergence.
- `POC_RESULTS.md` — measured results on the RTX 5070 Ti (Vulkan, true
  5120×2160, driver 595.71.05; ~66% GPU util / ~150W / 40°C / ~2 ms frame work
  under 20k motes + bloom; the GNOME/Mutter FPS-measurement caveat) and a
  framebuffer screenshot (`docs/poc-screenshot.png`).
- Stage 1 design spec (`docs/superpowers/specs/2026-06-17-stage1-ingestion-design.md`)
  and foundation implementation plan
  (`docs/superpowers/plans/2026-06-17-stage1-ingestion-foundation.md`).
- Project `README.md` and `CLAUDE.md`.
