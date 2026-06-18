# Orrery — POC Prompt (Stage 0 + synthetic render-loop slice)

## Context
You are bootstrapping `orrery`, a GPU-accelerated ambient visualization of live Claude Code agent activity. The full architecture and rationale live in `PLAN.md` at the repo root — **read it first**; it is the source of truth for the eventual system. This task is **only the first proof-of-concept**, not the full build. Do not implement anything outside the scope below.

Target machine: Ubuntu 26.04, GNOME on Wayland, NVIDIA RTX 5070 Ti (Blackwell) on the open-kernel driver.

## Objective
De-risk the two riskiest assumptions before committing to the full build:

1. **Hardware + render stack** — Bevy on wgpu (Vulkan) can render borderless-fullscreen at native 5K on Wayland and hold ~60 fps with a bloom postprocessing pass and a few hundred-to-thousand animated entities.
2. **Core architecture pattern** — a background data source can feed per-agent state into the render loop across a lock-free `triple_buffer` boundary, with the render loop reading the latest snapshot each frame and smoothly interpolating bursty changes into fluid motion. This proves the "agents as glowing nuclei reacting to state" concept from PLAN.md, using synthetic data.

## In scope
- A single Cargo binary crate using the **latest stable Bevy** (pin the exact version in `Cargo.toml`).
  - **IMPORTANT:** Bevy's API changes significantly between releases. Do **not** rely on memorized APIs — check the docs / migration guide for the version you pin and adapt. Window mode, bloom/HDR, frame-time diagnostics, and the render-adapter-info resource have all moved between versions.
- **Borderless fullscreen** on the current monitor. **Never** request exclusive fullscreen — Wayland no-ops it (per PLAN.md). Render at the surface's physical pixel size (respect fractional scaling); do not force a logical size.
- A synthetic agent-state source on a dedicated `std::thread` (**not** tokio yet — see exclusions), producing a small set of agents (default **16**, configurable) whose state evolves over time: status flips (idle/active/error), an `activity_level` random walk in `0..1`, a `token_rate`, a `model` tag, and occasional discrete "pulse" events. Make the output **bursty on purpose** so the smoothing is genuinely exercised.
- A lock-free **`triple_buffer`** (use the `triple_buffer` crate) as the **only** coupling between the synthetic source and the render world. The producer thread writes a `Vec<AgentState>` snapshot; a Bevy system reads the latest each frame. This seam is exactly where the real tokio ingestion layer will plug in later — keep it clean.
- Visual mapping (simple, but faithful to PLAN.md):
  - Each agent = a soft glowing nucleus (an HDR/additive sprite; bloom does the glow).
  - Nucleus size + brightness ∝ a **displayed** (interpolated) activity level.
  - Hue keyed by `model`.
  - Idle → gentle "breathing" (slow sine); active → faster "pulsing"; error → red tint + small wobble.
  - Discrete pulse events → a brief outward flare.
  - **Procedural motion only:** drift + spring-toward-home + a little noise. (No physics engine — see exclusions.)
- **Framerate-independent smoothing:** for every animated property keep a `target` (from data) and a `displayed` value, and each frame ease `displayed += (target - displayed) * (1 - (-dt / tau).exp())`. A burst of state changes must **glide, never teleport**.
- An ambient background field of instanced/batched sprites (default **~800 motes**, configurable) to actually load the GPU at 5K, mirroring PLAN.md's reference.
- **HDR camera + Bevy's built-in bloom.** Bloom must be **toggleable at runtime** (a key) so its 5K cost can be measured.
- An **on-screen diagnostics overlay** (and a startup stdout log) showing: live FPS and frame time (ms), entity count, the wgpu **backend** (expect Vulkan), the **adapter name** (expect the RTX 5070 Ti), the **driver / driver_info** string, and the actual surface resolution. Use Bevy's frame-time diagnostics + the render adapter info resource.
- Runtime controls: **Esc** to quit; a key to toggle bloom; keys to increase/decrease mote count (to probe the fps ceiling). A CLI flag or env var for initial agent count and mote count.

## Explicitly OUT of scope — do NOT build these
These are later stages, kept out deliberately to keep the POC tight:
- **No** real RabbitMQ, REST, or Mimir/PromQL. Synthetic source only. *(Stage 1)*
- **No** tokio runtime yet. The producer is a plain `std::thread`; tokio plugs in later at the `triple_buffer` seam. *(Stage 1)*
- **No** Rapier / physics engine. Use simple procedural motion. *(Stage 2)*
- **No** orbital bodies, inter-agent arcs, or dashboard text panels beyond the diagnostics overlay. *(Stage 2–3)*
- **No** idle detection, D-Bus, or screensaver integration. Just a normal fullscreen app launched from the terminal. *(Stage 4)*

## Suggested structure
Keep it lean — a few well-bounded modules:
- `src/main.rs` — Bevy app setup, window/fullscreen config, plugins, wiring.
- `src/agent.rs` — `AgentState` model + the synthetic producer thread.
- `src/sync.rs` — `triple_buffer` setup + the Bevy resource/system that reads the latest snapshot each frame.
- `src/visuals.rs` — nucleus entities, data→visual mapping, smoothing, motion, pulses, the bloom/HDR camera.
- `src/diagnostics.rs` — the on-screen overlay + startup logging of backend/adapter/driver/resolution.

## Acceptance criteria (verify and report each)
1. `cargo build --release` succeeds. Run `cargo clippy --all-targets` and report results (fix trivial lints; note anything intentional). No `unwrap()`/`expect()` on fallible startup paths that could panic silently — log and exit cleanly instead.
2. The app launches in **borderless fullscreen on the current monitor** under a **Wayland** session.
3. Startup stdout includes: backend (confirm it is **Vulkan**, not GL), adapter name, driver + driver_info, and the real surface resolution (confirm it matches the panel's native 5K). Capture these **exact strings**.
4. The on-screen overlay shows live FPS, frame time (ms), entity count, and backend.
5. At native 5K with default counts and **bloom on**, report the sustained FPS and frame time. State plainly whether ~60 fps is held; if not, report the actual numbers — **do not silently lower the resolution or entity counts to "pass."** Then report FPS with **bloom off** and at a couple of mote counts, to characterize the bloom / fill-rate cost at 5K.
6. Synthetic agents visibly change state and the nuclei react **smoothly** (interpolated, no teleporting), and discrete pulses produce visible flares — demonstrating the triple_buffer handoff + smoothing end to end.
7. Esc exits cleanly; bloom toggle and mote +/- work.

## Gotchas (from PLAN.md research — heed these)
- Borderless, **never** exclusive, fullscreen on Wayland.
- Bevy API churn: verify against the pinned version's docs; don't trust old examples.
- Bloom requires **HDR enabled** on the camera.
- If you hit a wgpu/Wayland `linux-drm-syncobj` "Surface already has a syncobj attached" error, note it and ensure winit/wgpu are current — it's a known edge case.
- Render at **physical pixels**; verify the logged resolution is true 5K, not a scaled logical size.

## When done
Write a short **`POC_RESULTS.md`** capturing: the observed backend / adapter / driver strings, the resolution, the FPS / frame-time numbers (bloom on/off, a couple of mote counts), confirmation the Wayland session was used, and any issues hit (especially driver / Wayland). This settles the driver question empirically and becomes the performance baseline for the full build.
