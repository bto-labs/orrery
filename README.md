# orrery

GPU-accelerated ambient visualization of live Claude Code agent activity.

Agents render as soft glowing nuclei (bloom does the glow), coloured by model,
breathing when idle and pulsing when active, over an ambient field of drifting
motes. The full design and rationale live in [`PLAN.md`](PLAN.md).

> **Status: Stage-0 proof of concept.** This is the first slice — it proves the
> render stack (Bevy/wgpu on Vulkan at native 5K on Wayland with bloom) and the
> core architecture (a background data source feeding the render loop across a
> lock-free `triple_buffer`, with bursty data smoothed into fluid motion) using
> **synthetic** agent data. See [`POC_RESULTS.md`](POC_RESULTS.md) for measured
> results on an RTX 5070 Ti. Real RabbitMQ/REST/Mimir ingestion, tokio, Rapier
> physics, and GNOME idle/D-Bus integration are later stages.

![orrery at 5120×2160](docs/poc-screenshot.png)

## Build & run

Requires a recent Rust toolchain and these system libraries (Debian/Ubuntu):

```bash
sudo apt-get install libwayland-dev libxkbcommon-dev libasound2-dev libudev-dev
```

```bash
# On Wayland, when launching from a non-graphical shell, point at the session:
export XDG_RUNTIME_DIR=/run/user/$(id -u) WAYLAND_DISPLAY=wayland-0

cargo run --release
```

The app opens **borderless fullscreen** on the current monitor. Press **Esc** to
quit.

## Controls

| Key | Action |
|---|---|
| `Esc` | quit |
| `B` | toggle bloom (to gauge its 5K cost) |
| `+` / `-` | add / remove background motes |

## Configuration

CLI flags (override env vars):

| Flag | Env var | Default | Meaning |
|---|---|---|---|
| `--agents N` | `ORRERY_AGENTS` | 16 | number of agent nuclei |
| `--motes N` | `ORRERY_MOTES` | 800 | ambient background motes |
| `--no-bloom` | `ORRERY_BLOOM=0` | bloom on | start with bloom disabled |
| `--no-vsync` | `ORRERY_VSYNC=0` | vsync on | uncap the frame rate |
| `--screenshot PATH` | `ORRERY_SCREENSHOT` | — | capture the framebuffer after warmup, then exit |
| | `ORRERY_SEED` | `0xC0FFEE` | RNG seed for the synthetic source |

## Architecture (this stage)

```
synthetic producer (std::thread, bursty)
        │  writes Vec<AgentState>
        ▼
   triple_buffer  ◄── the only coupling; tokio ingestion plugs in here later
        │  render loop reads latest each frame
        ▼
   Bevy ECS: smoothing (target → displayed) → nuclei + motes → HDR + bloom
```

Modules: `agent` (state model + producer), `sync` (triple_buffer seam),
`visuals` (nuclei, smoothing, motion, bloom camera), `diagnostics` (overlay +
startup render-stack log), `main` (app wiring, window config, controls).

## Tests

```bash
cargo test          # pure logic: random walk, pulse accounting, easing
cargo clippy --all-targets
```
