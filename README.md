# orrery

GPU-accelerated ambient visualization of live Claude Code agent activity.

Each active Claude Code session across the homelab renders as a soft glowing
nucleus (bloom does the glow), coloured by model, breathing when idle and
pulsing on activity, over an ambient field of drifting motes. Sessions fade in
on start and fade out on idle/stop. The full design and rationale live in
[`PLAN.md`](PLAN.md).

> **Status: Stage 1 — live data ingestion (shipped, v0.2.0).** orrery now
> visualizes real Claude Code sessions, ingested live from the homelab
> `claude-events` RabbitMQ exchange — session lifecycle/activity from `hook.#`,
> per-session model from `transcript.message` — through a tokio runtime → reducer
> → lock-free `triple_buffer` → Bevy render loop. The synthetic generator is
> retained as a `--synthetic` demo/dev mode. Stage 0 proved the render stack
> (Bevy/wgpu on Vulkan at native 5K on Wayland with bloom); see
> [`POC_RESULTS.md`](POC_RESULTS.md). Mimir and REST were evaluated and dropped
> (no `claude_code_*` metrics; no live REST feed) — see the §9 source-availability
> verification under `docs/superpowers/specs/`. Deferred to later stages:
> per-source health overlay, synthetic auto-fallback, `max_agents` enforcement,
> Rapier physics, GNOME idle/D-Bus integration.

## Build & run

Requires a recent Rust toolchain (1.96+) and these system libraries (Debian/Ubuntu):

```bash
sudo apt-get install libwayland-dev libxkbcommon-dev libasound2-dev libudev-dev
```

```bash
# On Wayland, when launching from a non-graphical shell, point at the session:
export XDG_RUNTIME_DIR=/run/user/$(id -u) WAYLAND_DISPLAY=wayland-0

# Live mode (default): set the broker URL, then run.
export RABBITMQ_URL='amqp://user:pass@rabbitmq.host:5672/'
cargo run --release

# Demo mode (no broker needed): synthetic agent field.
cargo run --release -- --synthetic
```

The app opens **borderless fullscreen** on the current monitor. Press **Esc** to
quit. In live mode with no `RABBITMQ_URL` set, orrery logs that the live source
is disabled and shows an empty field (motes only) until a broker is configured.

## Controls

| Key | Action |
|---|---|
| `Esc` | quit |
| `B` | toggle bloom (to gauge its 5K cost) |
| `+` / `-` | add / remove background motes |

## Configuration

Default mode is **live** (connect to RabbitMQ); `--synthetic` forces the demo
field. CLI flags override env vars:

| Flag | Env var | Default | Meaning |
|---|---|---|---|
| `--synthetic` | `ORRERY_SYNTHETIC=1` | off (live) | force the synthetic demo field (disables live sources) |
| `--no-rabbitmq` | — | on | disable the live hook source |
| `--no-transcript` | — | on | disable transcript model-enrichment (live nuclei stay neutral-hued) |
| `--idle-ms N` | `ORRERY_IDLE_MS` | 30000 | ms of no activity before a session goes idle |
| `--despawn-ms N` | `ORRERY_DESPAWN_MS` | 120000 | ms after a session stops before its nucleus despawns |
| `--max-agents N` | `ORRERY_MAX_AGENTS` | 64 | session cap (plumbed; enforcement deferred) |
| `--agents N` | `ORRERY_AGENTS` | 16 | synthetic session count (synthetic mode) |
| `--motes N` | `ORRERY_MOTES` | 800 | ambient background motes |
| `--no-bloom` | `ORRERY_BLOOM=0` | bloom on | start with bloom disabled |
| `--no-vsync` | `ORRERY_VSYNC=0` | vsync on | uncap the frame rate |
| `--screenshot PATH` | `ORRERY_SCREENSHOT` | — | capture the framebuffer after warmup, then exit |
| — | `RABBITMQ_URL` | — | AMQP broker URL (required for live mode) |
| — | `CLAUDE_EVENTS_EXCHANGE` | `claude-events` | topic exchange to bind |
| — | `ORRERY_SEED` | `0xC0FFEE` | RNG seed (synthetic source) |

## Architecture

```
  RabbitMQ ── hook.#───────────▶ rabbitmq source ─┐
 (claude-   ── transcript.message▶ transcript src ─┤
  events)                                          ├─▶ bounded mpsc ─▶ reducer ─┐
  [--synthetic] ───────────────▶ synthetic source ┘    (AgentUpdate)   owns the │
                                                         per-session model       │
                   tokio runtime (its own OS thread)                             │
                                                                                 ▼
                       triple_buffer  ◄── the ONLY coupling to the render world
                                                                                 │ Vec<AgentState>
                                                                                 ▼
           Bevy ECS: reconcile nuclei by session_id (spawn / fade / despawn) →
           smoothing (target → displayed) → motion → HDR + bloom
```

Each source task normalizes its input into an `AgentUpdate` and sends it over a
bounded channel; a single **reducer** owns the merged `HashMap<SessionId,
AgentState>` and is the only writer to the `triple_buffer`. The render side
reconciles a dynamic set of nuclei keyed by `session_id`. The reducer reads no
wall clock — all time arrives on update timestamps — so its lifecycle logic is
deterministic and unit-tested.

Modules: `ingest` (`model`, `reducer`, `sources/{rabbitmq,transcript}`,
`synthetic`, the tokio runtime + `triple_buffer` seam), `visuals` (dynamic
nuclei, smoothing, motion, HDR+bloom camera), `diagnostics` (overlay + startup
render-stack log), `main` (wiring / window / controls / config).

## Tests

```bash
cargo test          # model, reducer lifecycle, source parsers (fixture-based), easing
cargo clippy --all-targets
```
