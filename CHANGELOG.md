# Changelog

All notable changes to orrery are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions track `Cargo.toml`.

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
