# CLAUDE.md — orrery

GPU-accelerated ambient visualization of live Claude Code agent activity.
Rust + Bevy (wgpu/Vulkan). See `PLAN.md` (design source of truth),
`POC_RESULTS.md` (measured Stage-0 baseline), and `docs/superpowers/` (Stage 1
spec + plan).

## Build & run

- **Toolchain:** mise-managed Rust (1.96+). `cargo` resolves via mise shims.
- **System deps (Ubuntu):** `libwayland-dev libxkbcommon-dev libasound2-dev libudev-dev`.
- **Run on Wayland** (from a non-graphical shell, point at the live session):
  ```bash
  export XDG_RUNTIME_DIR=/run/user/$(id -u) WAYLAND_DISPLAY=wayland-0
  cargo run --release          # borderless fullscreen; Esc quits
  ```
- **Flags/env:** `--agents N` `--motes N` `--no-bloom` `--no-vsync`
  `--screenshot PATH`; `ORRERY_AGENTS/MOTES/SEED/BLOOM/VSYNC/SCREENSHOT`.
- **Controls:** Esc quit · B toggle bloom · +/- mote count.

## Architecture

- `triple_buffer` is the ONLY coupling between the data producer and the Bevy
  render world; the render loop reads the latest snapshot each frame and smooths
  it. Stage 1 replaces the synthetic `std::thread` producer with a tokio
  ingestion runtime (RabbitMQ + REST + Mimir → reducer → same `triple_buffer`).
- Modules: `agent` (model + synthetic producer), `sync` (seam), `visuals`
  (nuclei, smoothing, motion, HDR+bloom camera), `diagnostics` (overlay +
  startup log), `main` (wiring/window/controls). Stage 1 introduces `src/ingest/`.

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

## Target hardware

bto-storm: NVIDIA RTX 5070 Ti (Blackwell), driver 595.71.05, Ubuntu 26.04,
GNOME Wayland; primary panel 5120×2160 @ scale 1.0.

## Conventions

- No `unwrap()`/`expect()` on fallible startup or network paths — return
  `Result`, log, degrade. No panics in async ingestion tasks.
- Keep `cargo clippy --all-targets` clean and `cargo test` green.
