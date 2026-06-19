# Changelog

All notable changes to orrery are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions track `Cargo.toml`.

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
