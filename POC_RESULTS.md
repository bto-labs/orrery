# orrery — Stage-0 POC Results

Empirical results from the first proof-of-concept (see `docs/orrery-poc-prompt.md`
for the task and `PLAN.md` for the full design). The two goals were to de-risk
(1) the Bevy/wgpu render stack at native 5K on Wayland with bloom, and (2) the
`triple_buffer` seam + smoothing pattern using synthetic data.

**Verdict: both assumptions are de-risked.** The render stack runs on Vulkan at
true 5K with bloom on the target driver with no Wayland sync issues, the GPU is
nowhere near saturated, and the lock-free producer → render-loop → smoothing
pipeline works end to end.

![orrery POC at 5120×2160](docs/poc-screenshot.png)

## Test machine

| | |
|---|---|
| Host | `bto-storm` |
| OS / session | Ubuntu 26.04, GNOME on **Wayland** (Mutter), session live on tty2 |
| GPU | NVIDIA GeForce RTX 5070 Ti (Blackwell, 16 GB) |
| Driver | NVIDIA **595.71.05**, CUDA 13.2 |
| Toolchain | rustc/cargo **1.96.0** (mise-managed) |
| Bevy | **0.18.1** (latest stable; 0.19 is still RC) |
| Panel | DP-4, **5120×2160** ("5K2K" ultrawide), scale factor 1.0 |

> Note: the primary panel here is 5120×2160 (≈11.1 MP), not the 5120×2880 the
> prompt anticipated. The app renders at the surface's true physical size and
> reports it honestly; all numbers below are at 5120×2160.

## 1. Render stack — exact startup strings

Captured verbatim from stdout (`bevy_render::renderer` and `orrery::diagnostics`):

```
AdapterInfo { name: "NVIDIA GeForce RTX 5070 Ti", vendor: 4318, device: 11269,
              device_type: DiscreteGpu, driver: "NVIDIA",
              driver_info: "595.71.05", backend: Vulkan }

== orrery render stack ==
  backend     : Vulkan
  adapter     : NVIDIA GeForce RTX 5070 Ti
  driver      : NVIDIA
  driver_info : 595.71.05
  resolution  : 5120x2160 (physical)  scale 1.000
  logical     : 5120x2160
```

- **Backend is Vulkan**, not GL. ✔
- **Adapter is the RTX 5070 Ti.** ✔
- **Resolution is true physical 5120×2160** at scale 1.0 (no fractional-scaling
  surprise on this monitor). ✔
- Wayland backend confirmed: winit used the Smithay client toolkit
  (`sctk_adwaita` appears in the logs, which is Wayland-only).

## 2. Performance

### Headline: the GPU is barely working at 5K

The most reliable signal is **GPU telemetry under heavy load**, sampled with
`nvidia-smi dmon` during a stress run of **20,000 motes + bloom, uncapped, at
5120×2160**:

| metric | value |
|---|---|
| SM utilization | **58–72 %** (≈66 % typical) |
| Memory-controller util | 45–56 % |
| Board power | **145–154 W** of a 300 W cap |
| Temperature | **38–41 °C** |
| Core clock | 2842 MHz (boosting) |
| VRAM in use | ~3.2 GB |
| App render-loop frame time | **~2 ms** |

Even at 20k motes + bloom — far beyond the ~800-mote default — the card sits at
~two-thirds SM utilization, half its power budget, and 40 °C, with ~2 ms of work
per frame. The 60 fps budget is 16.6 ms, so there is roughly **8× headroom**.
**~60 fps is held trivially.** ✔

No `linux-drm-syncobj` "Surface already has a syncobj attached" error occurred —
the known wgpu/Wayland edge case did not bite on this driver.

### Caveat: wall-clock FPS is not a clean benchmark on the live GNOME desktop

The per-frame *FPS* numbers Bevy reports are **unreliable as a workload
measurement here**, and it's important to say so rather than present misleading
figures. Running as a normal (non-exclusive) Wayland surface, **Mutter governs
presentation** — it alternates between direct-scanout and full compositing
depending on what else is damaging the screen — so the app's render-loop rate is
gated by irregular compositor buffer-release timing, not by GPU cost.

The symptom: across runs the reported FPS swung wildly (≈70–650 fps) and scaled
*impossibly* with load. For example, median uncapped FPS (16 s runs):

| config (uncapped, 5120×2160) | median FPS | note |
|---|---:|---|
| bloom on, 800 motes | 272 | |
| bloom **off**, 800 motes | 233 | "slower" than bloom on — impossible |
| bloom on, 3000 motes | 300 | |
| bloom on, 8000 motes | 564 | "faster" with 10× the motes — impossible |
| vsync on, 800 motes | 120 | least noisy, ≈ panel refresh |

These orderings are physically impossible for the actual rendering workload,
which is the proof that the variance is compositor scheduling, not the GPU.
**What is trustworthy:** under *every* condition tested (800 → 40,000 motes,
bloom on/off, vsync on/off) the rate never approached 60 fps from above, and the
GPU telemetry + ~2 ms frame work show the render cost is single-digit
milliseconds throughout.

**For rigorous FPS / bloom-cost numbers in the full build**, measure
GPU-side time with wgpu timestamp queries (independent of the compositor),
and/or benchmark from a quiescent/dedicated session. Consider enabling VRR.
This was out of scope for de-risking, which the telemetry already settles.

## 3. The triple_buffer + smoothing pipeline

- A dedicated `std::thread` (no tokio) runs the synthetic producer and publishes
  `Vec<AgentState>` snapshots through a `triple_buffer` (the `triple_buffer`
  crate, v9). A single Bevy system reads the latest snapshot each frame. This is
  the only coupling between the data source and the ECS — exactly where the real
  tokio ingestion layer will plug in later.
- The producer is intentionally **bursty** (occasional "storm" ticks, large
  jumps, randomized cadence with stalls and flurries).
- The render side keeps a `target` and a `displayed` value per animated property
  and eases with the framerate-independent
  `displayed += (target − displayed) · (1 − e^(−dt/τ))`. Bursts **glide, never
  teleport**. Discrete pulses cross the buffer as a monotonic counter that the
  render side diffs, so pulses survive the lossy latest-only read (never missed,
  never double-counted).
- Unit tests (`cargo test`, 8 passing) lock in the load-bearing logic: the
  random walk stays in `0..=1` under 5,000 bursty ticks, pulse counts are
  monotonic, the producer is deterministic per seed, and the easing converges
  monotonically without overshoot.
- Visually confirmed from the screenshot: nuclei glow with bloom, are coloured
  by model (violet = Opus, azure = Sonnet, green = Haiku), one agent is in the
  **red Error** state, sizes vary by activity, and the ambient mote field loads
  the background.

## 4. Acceptance criteria

| # | Criterion | Status |
|---|---|---|
| 1 | `cargo build --release` succeeds; clippy clean; no panic-prone `unwrap`/`expect` on fallible startup | ✔ (thread spawn returns `Result`, handled with clean exit; clippy `--all-targets` clean) |
| 2 | Borderless fullscreen on current monitor under Wayland | ✔ |
| 3 | Startup logs backend (Vulkan) / adapter / driver / driver_info / true resolution | ✔ (exact strings above) |
| 4 | On-screen overlay shows FPS, frame time, entity count, backend | ✔ (visible in screenshot) |
| 5 | Report sustained FPS at 5K, bloom on/off, a couple of mote counts | ✔ reported honestly: 60 fps held with large margin; precise per-config FPS unreliable on live Mutter (documented, with telemetry as the real evidence) |
| 6 | Agents change state; nuclei react smoothly; pulses flare | ✔ (unit-tested smoothing/burstiness + visual confirmation) |
| 7 | Esc quits cleanly; bloom toggle and mote +/- work | ✔ verified with **real OS keystrokes** injected via `ydotool` (kernel uinput → Mutter → Bevy input → controls): `B` toggled bloom OFF then ON, `=` grew motes 800→900→1000, `-` shrank 1000→900, `Esc` exited cleanly (exit code 0) |

## 5. Issues encountered

- **Missing system dev libraries** for the build/Wayland backend, installed:
  `libasound2-dev`, `libudev-dev`, `libwayland-dev`, `libxkbcommon-dev`.
- **`Bevy 0.18` API churn vs. memory** (handled by checking the pinned version's
  source/docs): HDR is now a separate `Hdr` marker component (not
  `Camera { hdr: true }`); bloom moved to `bevy::post_process::bloom`; `AppExit`
  is a `Message` so it needs `MessageWriter` (not `EventWriter`);
  `WindowMode::BorderlessFullscreen` now takes a `MonitorSelection`.
- **winit/Wayland warning** `Can't select current monitor on window creation or
  cannot find current monitor!` (the PLAN.md gotcha, bevy #18556). Benign here —
  the window still came up borderless-fullscreen at the native 5120×2160.
- **`sctk_adwaita: Ignoring unknown button type`** — harmless client-side
  decoration warning.
- **`wayland` is not a Bevy default feature** (x11 is); it must be enabled
  explicitly or the app silently falls back to XWayland. Enabled in `Cargo.toml`.

## 6. How to reproduce

```bash
# Wayland session env (when launching from a non-graphical shell):
export XDG_RUNTIME_DIR=/run/user/$(id -u) WAYLAND_DISPLAY=wayland-0

cargo run --release                       # 16 agents, 800 motes, bloom + vsync
cargo run --release -- --motes 3000       # heavier ambient field
cargo run --release -- --no-bloom         # bloom off
cargo run --release -- --no-vsync         # uncapped (compositor-noisy; see §2)
cargo run --release -- --screenshot out.png   # capture framebuffer, then exit

# env equivalents: ORRERY_AGENTS, ORRERY_MOTES, ORRERY_SEED, ORRERY_BLOOM=0,
#                  ORRERY_VSYNC=0, ORRERY_SCREENSHOT=out.png
```

Runtime controls: **Esc** quit · **B** toggle bloom · **+/-** add/remove motes.
