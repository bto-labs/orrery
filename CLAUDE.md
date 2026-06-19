# CLAUDE.md — orrery

GPU-accelerated ambient visualization of live Claude Code agent activity.
Rust + Bevy (wgpu/Vulkan). **Stage 1 shipped (v0.2.0): live RabbitMQ ingestion.**
See `PLAN.md` (design source of truth), `POC_RESULTS.md` (Stage-0 render
baseline), and `docs/superpowers/` (Stage 1 + Plan 2 specs/plans, and the §9
source-availability verification).

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

## Target hardware

bto-storm: NVIDIA RTX 5070 Ti (Blackwell), driver 595.71.05, Ubuntu 26.04,
GNOME Wayland; primary panel 5120×2160 @ scale 1.0.

## Conventions

- No `unwrap()`/`expect()` on fallible startup or network paths — return
  `Result`, log, degrade. No panics in async ingestion tasks.
- Keep `cargo clippy --all-targets` clean and `cargo test` green.
