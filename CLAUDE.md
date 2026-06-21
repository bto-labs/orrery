# CLAUDE.md — orrery

GPU-accelerated ambient visualization of live Claude Code agent activity.
Rust + Bevy (wgpu/Vulkan). **Stage 1 shipped (v0.2.0): live RabbitMQ ingestion.**
**Stage 2 in progress — character avatars:** Subsystem A (the avatar-generation
pipeline, `tools/avatar-gen/`) has shipped (avatar-gen v0.1.0); Subsystem B (the
renderer rework that consumes the generated frames) is not built yet.
See `PLAN.md` (design source of truth), `POC_RESULTS.md` (Stage-0 render
baseline), and `docs/superpowers/` (Stage 1 + Plan 2 specs/plans, the §9
source-availability verification, and the Stage-2 character-avatars design +
Subsystem-A plan).

## Build & run

- **Toolchain:** mise-managed Rust (1.96+). `cargo` resolves via mise shims.
- **System deps (Ubuntu):** `libwayland-dev libxkbcommon-dev libasound2-dev libudev-dev`.
- **Run on Wayland** (from a non-graphical shell, point at the live session):
  ```bash
  export XDG_RUNTIME_DIR=/run/user/$(id -u) WAYLAND_DISPLAY=wayland-0
  cargo run --release          # borderless fullscreen; Esc quits
  ```
- **Live by default** — connects to RabbitMQ; needs `RABBITMQ_URL` in env
  (OpenBao-injected / `~/.env` on bto-storm) + optional `CLAUDE_EVENTS_EXCHANGE`
  (default `claude-events`). `--synthetic` (or `ORRERY_SYNTHETIC=1`) forces the
  demo field and disables live sources.
- **Flags/env:** `--synthetic` `--no-rabbitmq` `--no-transcript` `--idle-ms N`
  `--despawn-ms N` `--max-agents N` `--agents N` `--motes N` `--no-bloom`
  `--no-vsync` `--screenshot PATH`; `ORRERY_{SYNTHETIC,IDLE_MS,DESPAWN_MS,
  MAX_AGENTS,AGENTS,MOTES,SEED,BLOOM,VSYNC,SCREENSHOT}`, `RABBITMQ_URL`,
  `CLAUDE_EVENTS_EXCHANGE`.
- **Controls:** Esc quit · B toggle bloom · +/- mote count.

## Architecture

- `triple_buffer` is the ONLY coupling between ingestion and the Bevy render
  world; the render loop reads the latest snapshot each frame and smooths it.
- **Stage 1 (shipped):** a tokio runtime on its own OS thread runs source tasks
  that each normalize input into an `AgentUpdate` and send it over a bounded
  `mpsc`; a single **reducer** owns the merged `HashMap<SessionId, AgentState>`
  and is the *only* writer to `triple_buffer`. Live sources: `rabbitmq` (`hook.#`
  → lifecycle/activity/attention) and `transcript` (`transcript.message` → model);
  `synthetic` is retained as `--synthetic`. **The reducer reads no wall clock** —
  all time arrives on update timestamps (keeps it deterministic + unit-testable).
  **Mimir + REST were dropped** (§9: no `claude_code_*` metrics; no live REST
  feed). Source tasks NEVER touch the Bevy world or write the buffer.
- Modules: `ingest` (`model`, `reducer`, `sources/{rabbitmq,transcript}`,
  `synthetic`, runtime + seam in `mod.rs`), `visuals` (dynamic nuclei keyed by
  `session_id` w/ spawn/fade/despawn, smoothing, motion, HDR+bloom camera),
  `diagnostics` (overlay + startup log), `main` (wiring/window/controls/config).
  (`agent.rs` + `sync.rs` were removed — superseded by `ingest`.)

## Stage 2 — avatar-gen (Subsystem A, `tools/avatar-gen/`)

- **Separate package, not the Rust crate.** A standalone Node 20+/TypeScript worker
  (npm, vitest) under `tools/avatar-gen/`; it is excluded from `cargo` and versioned
  independently (`avatar-gen` v0.1.0). Run its commands from `tools/avatar-gen/`
  (`npm test`, `npm run typecheck` runs **both** `tsc` passes — `src` and `test`).
- **Pipeline:** per-repo metadata (auto-derived from git remote + gitea, with curated
  overrides) → one Gemini contact-sheet (base bots as reference images, 5 poses in a
  single generation — the consistency trick) → `sharp` alpha-gutter slice into 5
  frames → SeaweedFS upload → `assets/agents/registry.json`. Build A before B: its
  cached frames are the interface B consumes.
- **Every external boundary is an injected interface** (`ImageGenerator`, `FrameStore`,
  `GiteaClient`, `GitInspector`, `now()`), so pure logic is unit-tested with fakes; the
  live Gemini call is a **gated spike** (`test/integration.live.test.ts`, runs only when
  `GEMINI_API_KEY` is set). The orchestrator reads no wall clock (time via injected
  `now()`) — same discipline as the Stage-1 reducer.
- **The 5 canonical poses are a fixed cross-subsystem contract:** `neutral, idle,
  active, attention, error`. Don't rename/reorder without updating the renderer.
- **Cache key = `repoKey + metadataHash`;** `metadataHash` hashes identity + curated
  fields only (volatile `createdAt`/`ageDays`/`hosts` and all generation bookkeeping
  excluded, so age churn / prior output never trigger regeneration; `accentPalette`
  order is significant, other arrays are normalized).
- **Frame-URI contract (A→B), spec §3.1:** frames live at
  `s3://<bucket>/<repoKey>/<metadataHash>/<pose>.png`, where `repoKey` is itself a
  multi-segment unencoded id (`gitea.bto.bar/BTO/orrery`). Subsystem B must locate
  frames by reconstructing the key from the discrete `repoKey`+`metadataHash`+`pose`
  fields in the registry — **not** by `split('/')`-ing the opaque `uri`.

## Gotchas (hard-won)

- **Bevy 0.18 API churn — verify against the pinned version, don't trust memory:**
  HDR is a separate marker component `bevy::render::view::Hdr` (not
  `Camera{hdr:true}`); bloom is at `bevy::post_process::bloom`; `AppExit` is a
  `Message` → use `MessageWriter` (not `EventWriter`);
  `WindowMode::BorderlessFullscreen(MonitorSelection::Current)`.
- **`wayland` is NOT a Bevy default feature** (x11 is). It must be enabled in
  `Cargo.toml` or the app silently falls back to XWayland.
- **FPS benchmarking is unreliable on the live GNOME/Mutter desktop** — Mutter
  governs presentation for a normal windowed surface (direct-scanout vs
  composite), so app-measured FPS swings wildly and doesn't track workload. Use
  **GPU telemetry** (`nvidia-smi dmon`) and/or **wgpu timestamp queries** for
  real numbers; the GPU has huge headroom (~2 ms frame work at 5K). See
  `POC_RESULTS.md` §2.
- Never request **exclusive** fullscreen on Wayland (no-op/panic). Render at
  physical pixels; the winit "Can't select current monitor" warning is benign.
- **lapin 4.10 ≠ older docs (verify against the installed crate):** no
  `ConnectionProperties::with_executor`/`with_reactor` — it auto-detects the
  tokio runtime (so the `tokio-executor-trait`/`tokio-reactor-trait` crates are
  NOT needed); the consumer `Stream` yields `Result<Delivery>` (not
  `Result<Option<Delivery>>`); queue/exchange/key params need `.into()` to
  `ShortString`.
- **Empty AMQP vhost ≠ "/" in lapin.** `RABBITMQ_URL` ends in `…:5672/` (empty
  vhost); Node's amqplib (the other claude-events consumers) silently maps empty
  → "/", but lapin connects to vhost `""` → `NOT_ALLOWED - vhost not found`. We
  default an empty vhost to "/" via `amqp_uri_with_default_vhost` + `connect_uri`.
- **`hook.*` carries no model; `claude_code_*` metrics don't exist in Mimir**
  (§9). Per-session model comes only from `transcript.message` (assistant turns,
  `.message.model`). `Summary{model}` is **enrichment-only** in the reducer
  (update-if-present) — a model arriving before `SessionStart`, or replayed after
  a reconnect, must NOT seed a ghost session (`last_activity_ms=0` → flips Idle
  forever, never despawns).
- **orrery declares its own durable queues** `orrery.hook`/`orrery.transcript`
  bound to `hook.#`/`transcript.message`, bounded with `x-message-ttl` +
  `x-max-length` so a stopped orrery can't grow the shared broker. Changing the
  declare args requires deleting the existing queues first (else
  `PRECONDITION_FAILED`).
- **avatar-gen uses `@google/genai` (the official TS SDK), NOT a Rust crate** —
  community Rust Gemini crates were too immature; the native `generateContent`
  image path is the reliable one. Image API: `ai.models.generateContent({model,
  contents:[...refParts,{text}], config:{imageConfig:{aspectRatio,imageSize}}})`;
  output at `response.candidates[0].content.parts[].inlineData.data` (base64).
- **Gemini image model id churns — confirm at build.** Default is
  `gemini-3-pro-image-preview` (Nano Banana Pro — strongest character consistency,
  chosen since generation is one-time/cached so quality beats latency); override via
  `AVATAR_MODEL_ID`. The `-preview` suffix and family names change; verify against
  the live model list before the first real spike run.
- **avatar-gen secrets are env-only (OpenBao-injected):** `GEMINI_API_KEY`,
  `SEAWEEDFS_S3_*`, `GITEA_TOKEN` — never hardcoded/logged; config errors list var
  NAMES only. `derive` and `generate --dry-run` need only the Gitea subset; the full
  Gemini/S3 set is required only on the real `generate`.

## Target hardware

bto-storm: NVIDIA RTX 5070 Ti (Blackwell), driver 595.71.05, Ubuntu 26.04,
GNOME Wayland; primary panel 5120×2160 @ scale 1.0.

## Conventions

- No `unwrap()`/`expect()` on fallible startup or network paths — return
  `Result`, log, degrade. No panics in async ingestion tasks.
- Keep `cargo clippy --all-targets` clean and `cargo test` green.
