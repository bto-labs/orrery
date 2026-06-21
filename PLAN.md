# Building a GPU-Accelerated Ambient Agent Visualization on Ubuntu 26.04 + RTX 5070 Ti

> **Status (2026-06-20):** Stage 0 (render-stack POC) ✓ and Stage 1 (live data
> ingestion) ✓ shipped (v0.2.0). Live ingestion is RabbitMQ-only (hook events +
> transcript-message for model); **Mimir and REST were evaluated and dropped**
> per the §9 source-availability verification (`docs/superpowers/specs/`). Rapier
> physics, orbital bodies, and GNOME idle/D-Bus integration remain future stages.
>
> **Stage 2 — visual direction revised to representational character avatars.**
> The abstract "glowing nucleus + orbital bodies" metaphor below is being replaced:
> each agent renders as a **bot character** themed per repo, with live `hook.*`
> events as **activity icons orbiting** it, and human-in-the-loop sessions as a
> human+bot pair. See `docs/superpowers/specs/2026-06-19-stage2-character-avatars-design.md`.
> Stage 2 decomposes into **Subsystem A — the avatar-generation pipeline ✓ shipped
> (avatar-gen v0.1.0, `tools/avatar-gen/`)** — and **Subsystem B — the renderer
> rework (not built yet)**, which keeps Stage 1's ingestion seam and lands the
> deferred Rapier/force-field motion + the orbital activity icons.
>
> This document is the original research/design source-of-truth; see
> `docs/superpowers/` for the per-stage specs/plans and `CHANGELOG.md` for what shipped.

## TL;DR
- **Build it in Rust on wgpu with the Bevy game engine, using Bevy's ECS + Rapier 2.5D physics, a decoupled tokio data layer (lapin for RabbitMQ, reqwest for REST + Mimir PromQL), and run it as a standalone borderless-fullscreen app triggered by GNOME's `org.gnome.Mutter.IdleMonitor` D-Bus watch.** This beats the web/Electron stack of the reference Observatory project for a long-running, native 5K screensaver while keeping the same proven visual language (soft glowing nuclei + orbital bodies + bloom).
- **Decouple ingestion from rendering with a lock-free SPSC ring buffer / triple buffer**: tokio threads consume RabbitMQ/REST/Mimir and write the latest per-agent state into a shared snapshot the render loop reads each frame; smooth bursty data with exponential interpolation so visuals never stall the frame and never jump.
- **The biggest gotcha is NVIDIA Blackwell on Linux**: the RTX 5070 Ti requires the open-kernel driver module (`nvidia-driver-570-open` or newer, ideally 575/580+) and the explicit-sync protocol that landed in the stable NVIDIA 555.58 driver (released June 27, 2024) for flicker-free Wayland. Plan for this in your environment setup before writing a line of rendering code.

## Key Findings

### What the reference project (Observatory) teaches
Observatory is a TypeScript + React + react-three-fiber (three.js/WebGL) app with bloom/vignette/chromatic-aberration postprocessing, wrapped in a WKWebView for iOS. Its architecture is genuinely worth borrowing — but its *implementation stack* (web/Electron-style) is the wrong choice for a long-running native 5K Linux screensaver. The borrow-worthy ideas:

1. **A clean four-layer separation**: Ingest (adapters) → State (reducer) → Render (scene) → Presentation. This is framework-agnostic and maps perfectly onto Rust.
2. **A single canonical internal event schema.** Observatory normalizes every source into one `AgentEvent` type (`tool.invoke`, `file.read`, `task.start`, `heartbeat`, etc. — eleven types, one schema). You should do exactly this: normalize RabbitMQ JSONL, REST summaries, and Mimir metrics into one internal `AgentEvent`/`AgentState` model keyed by agent ID.
3. **The visual metaphor**: each agent = a soft glowing nucleus; tools/files/memory/subtasks = orbital bodies; inter-agent calls = bezier arcs with flowing particles; idle = "breathing," active = "pulsing." This is a strong, legible ambient design you can reproduce.
4. **Performance discipline**: their per-frame `tick()` mutates state in place and never triggers React re-renders; halo textures are module-scope singletons; device-pixel-ratio capped at 2 because "beyond 2× the bloom pass cost dwarfs any visual gain." The Rust/ECS equivalent: keep hot-path per-frame updates allocation-free, instance your sprites, and budget your postprocessing passes.
5. **A "performance fallback ladder"**: when frame time exceeds budget, reduce particle count → orbital body density → disable bloom; a reduced-motion path drops particles to 320 and replaces pulses with static intensity. Build this in from day one.

The key divergence: Observatory itself notes its spec called for a "Rust core compiled to wasm + native dylib" and a native macOS `.saver` bundle, but it shipped a web bundle instead as a hackathon shortcut. You are effectively building the native version the spec originally envisioned.

### 1. Rendering stack and language — recommendation: Rust + wgpu, via Bevy

| Option | 5K + many entities | NVIDIA/Linux maturity | RabbitMQ + HTTP | Physics | Long-running ergonomics | Verdict |
|---|---|---|---|---|---|---|
| **Rust + wgpu/Bevy** | Excellent — Vulkan backend, GPU instancing, compute shaders | Very good — wgpu defaults to Vulkan on Linux/NVIDIA | Excellent — lapin, reqwest, tokio | Rapier (first-class Bevy plugin) | Excellent — single static binary, low memory, no GC | **Recommended** |
| C++ + Vulkan/OpenGL | Excellent — maximum control | Excellent — most mature | Good — librabbitmq, libcurl, but more manual | Jolt, Bullet, Box2D | Good but high effort; manual memory mgmt | Strong but slower to build |
| Go + Ebiten/go-gl | Ebiten is 2D-only, OpenGL-based, batches sprites well but not aimed at 5K compute-heavy generative art | Moderate — OpenGL path, cgo for bindings | Excellent — amqp091-go, net/http | No mature native physics; would port Box2D | Excellent — but GC pauses risk frame hitches | Not recommended for this |
| Web/WebGPU (three.js/Electron) | Good with WebGPU + instancing, but JS main-thread tax, and WebGPU on three.js can be *slower* than WebGL for many un-batched meshes | WebGPU on Linux/NVIDIA still maturing | Good — ws/amqp libs, fetch | Rapier (WASM), Matter.js, cannon | Electron is heavy (RAM, a whole Chromium) for a 24/7 screensaver | Fastest to prototype, worst long-term fit |

**Why Rust + wgpu via Bevy wins for this specific job:**
- **wgpu targets Vulkan natively on Linux/NVIDIA** and is the same architecture Bevy is built on; it gives "fast low-level rendering backed by the best API for the platform" with a far more approachable API than raw Vulkan. Bevy defaults to Vulkan on Linux (OpenGL is an unsupported fallback).
- **Bevy's ECS is an ideal fit** for "N agents, each with M orbital bodies, each with physics + visual state." Entities/components map directly onto your data model, and the scheduler parallelizes update systems across cores.
- **Single self-contained binary, no garbage collector** — critical for a process that runs for days. No GC pauses means no periodic frame hitches, unlike Go or JS.
- **The data ecosystem is excellent**: `lapin` (async AMQP/RabbitMQ on tokio), `reqwest` (HTTP), `serde_json` (JSONL parsing) are all mature and async-native.
- **Honest caveat on Go**: the user was considering Go, but Go's best 2D engine, Ebiten, is explicitly a 2D sprite engine (OpenGL, 4096×4096 texture atlas, automatic batching) with no native physics engine and a GC that can introduce frame-timing jitter in a sustained high-framerate loop. It's a fine tool for simpler 2D games; it is the wrong tool for compute-shader-driven generative art at 5K. Go is excellent for the *data/back-end* side, so a viable hybrid is a Go data service feeding a Rust renderer — but that adds an IPC boundary for no real benefit when Rust's data libraries are already first-class. **Commit to Rust end-to-end.**

**Alternative if the team has zero Rust experience and deep C++ graphics experience:** C++ + Vulkan with Jolt physics is technically equally capable and more mature, at the cost of much more boilerplate and manual safety. But for a greenfield project, Rust/Bevy gets you to a polished result faster with fewer footguns.

### 2. Physics engine — Rapier, in a "2.5D" configuration

**Use Rapier (`rapier2d`).** Rapier is the mature, pure-Rust, performance-first physics engine from Dimforge. Per Dimforge's announcement: "In release mode, Rapier runs 5 to 8 times faster than nphysics, making it close to the performance of (the CPU version of) NVidia PhysX and slightly faster than Box2D." It supports SIMD and rayon parallelism, and has a first-class official Bevy plugin (`bevy_rapier`). It handles exactly the behaviors you want: force-based joints (attraction/tethering of orbital bodies to nuclei), collisions, and scene queries. (Note: enable `parallel` only if the scene is sufficiently complex — Dimforge warns parallelism overhead can make small simulations *slower*.)

**2D vs 3D — recommendation: render in pseudo-3D but simulate in 2D ("2.5D").** The Observatory aesthetic (nuclei, orbital bodies, arcs) reads as a flat "galactic graph," and the legibility you need for dashboard overlays strongly favors a 2D plane. 2D physics is dramatically cheaper, more deterministic, and easier to keep legible at 5K with hundreds of bodies. Use `rapier2d` for motion (floating, attraction/repulsion via forces, soft jitter, collision avoidance) and add depth *visually* through scale, parallax, bloom, and z-layering. Reserve true `rapier3d` only if you later want a genuinely volumetric galaxy.

For the floating/attraction/repulsion "live avatar" feel, you often don't even need full rigid-body collision: a lightweight **boids/force-field model** (springs toward home position + inter-agent repulsion + noise-driven wander) driven either by Rapier forces or a custom system can look more organic than hard collisions. Use Rapier where you want genuine collision/containment, and use a GPU compute-shader particle system (wgpu/Bevy compute) for the *background* motes (Observatory uses 850 vertex-shader-driven motes) — GPUs handle hundreds of thousands of independent particles trivially, since each particle's physics depends only on its own data plus shared globals.

### 3. Real-time data ingestion architecture

**Core principle: never let I/O touch the render thread.** Run all networking on tokio worker threads; the render loop only ever reads a ready-made snapshot.

**The three-source merge, keyed per agent:**
```
                 ┌───────────────────────────────────────────┐
  RabbitMQ  ─────▶ lapin consumer (tokio)  ─┐                 │
 (durable topic,                            │                 │
  JSONL/agent)                              ▼                 │
                                    ┌──────────────────┐      │
  REST summaries ─▶ reqwest poller ─▶  Ingest/Reducer  │      │
  (HTTP, ~1–5s)                     │  HashMap<AgentId, │      │
                                    │   AgentState>     │      │
  Mimir/Prometheus ▶ reqwest PromQL ▶  (authoritative   │      │
  (instant query,    poller (~5–15s)│   merged model)   │      │
   token/model/cost)                └─────────┬─────────┘      │
                                              │ write          │
                                    ┌─────────▼─────────┐      │
                                    │ triple-buffer /   │◀─────┘
                                    │ SPSC ring buffer  │
                                    └─────────┬─────────┘
                                              │ read (lock-free, per frame)
                                    ┌─────────▼─────────┐
                                    │  Bevy render loop │  60–144 fps
                                    │ interpolate→draw  │
                                    └───────────────────┘
```

**Patterns to use:**
- **Decoupling**: a single-producer/single-consumer triple buffer (e.g. the `triple_buffer` crate) or a bounded ring buffer is the canonical game-engine pattern — the simulation thread writes to one index while the render thread reads another, lock-free. The render loop grabs the most recent published snapshot each frame; it never blocks on a mutex held by the network layer.
- **Bounded queues + back-pressure**: give the RabbitMQ consumer a bounded channel (tokio `mpsc` with capacity). Use a modest QoS prefetch on the AMQP channel so RabbitMQ doesn't flood you, and **ack only after parsing**. If the bound is hit, drop or coalesce oldest per-agent events (you want *latest state*, not every historical event, for a visualization) — this is intentional lossy back-pressure appropriate for ambient visuals.
- **Bursty → smooth**: store both a *target* value and a *displayed* value per visual property. Each frame, ease displayed toward target with a framerate-independent exponential smoothing (`displayed += (target - displayed) * (1 - exp(-dt/tau))`). A burst of 50 JSONL messages becomes a smooth glide, not a teleport. Observatory does the analogous thing with its 800 ms entry / 1200 ms exit / 700 ms pulse easings — copy that timing language.
- **RabbitMQ specifics**: use `lapin` with `ConnectionProperties::default().enable_auto_recover()` so the consumer transparently reconnects and replays topology (exchanges, queues, bindings, consumers) after a network blip — essential for a 24/7 process. Bind a durable queue to your topic exchange with the per-agent/per-project routing keys; `basic_consume` yields an async stream of `Delivery` values you ack explicitly.
- **REST polling**: a simple tokio interval task hitting the summary endpoint every 1–5 s; merge results into the shared model by agent ID.
- **Mimir/OTel enrichment**: Grafana Mimir exposes a **Prometheus-compatible HTTP API**. Use the instant-query endpoint `GET/POST <prometheus-http-prefix>/api/v1/query?query=<PromQL>` with the `X-Scope-OrgID` header for the tenant (required when multi-tenancy is on). Poll PromQL like `sum by (model)(rate(claude_code_token_usage_tokens_total[1m]))` every 5–15 s — telemetry export is coarse anyway: per Anthropic's docs, "metrics export every 60 seconds and traces and logs export every 5 seconds" by default, and SigNoz recommends setting `OTEL_METRIC_EXPORT_INTERVAL=10000` to flush every 10 s. **Naming gotcha**: confirmed by tcude.net's setup writeup, without the `opentelemetry.usePrometheusNaming` flag the metric stays as dot-form `claude_code.token.usage`, but PromQL dashboards "expect Prometheus-style names, like `claude_code_token_usage_tokens_total`." **Temporality gotcha**: Claude Code defaults to Delta temporality — set `OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE=cumulative` on the exporter side or short sessions can vanish before Prometheus scrapes them. Key the result by the `session.id`/agent attribute and merge into the same per-agent state.

**Merge model**: maintain `HashMap<AgentId, AgentState>` as the single source of truth. RabbitMQ events drive discrete actions/animations (a tool fired → spawn an orbital body, a pulse). REST summaries set coarse status (active/idle, current task). Mimir metrics set continuous enrichment fields (token rate, model, cost). Each writes only its own fields; the render layer reads the unified struct.

### 4. Screensaver / fullscreen idle integration on Ubuntu 26.04 (GNOME + Wayland)

This is the area with the most Linux-specific traps. Findings:

- **Do NOT build an XScreenSaver hack or a Wayland `ext-idle-notify-v1` client for GNOME.** Research confirms **Mutter (GNOME's compositor) does not implement the standard Wayland idle-notification protocol `ext-idle-notify-v1`** (nor the older `org.kde.kwin.idle`) as of GNOME 48/49 in 2025 — only KWin, Sway, and wlroots-based compositors implement it. So Wayland-native idle daemons like `swayidle` will not work on GNOME. (Mutter *does* support the separate `zwp_idle_inhibit_manager_v1` *inhibit* protocol, but that prevents idle, it doesn't detect it.)
- **The correct mechanism on GNOME/Wayland is the D-Bus interface `org.gnome.Mutter.IdleMonitor`** — the same interface GNOME's own screensaver/lock stack (gnome-settings-daemon's gsd-power) uses. The full interface (from Mutter's `org.gnome.Mutter.IdleMonitor.xml`):
  - Bus name `org.gnome.Mutter.IdleMonitor`, object path `/org/gnome/Mutter/IdleMonitor/Core`, interface `org.gnome.Mutter.IdleMonitor`.
  - `GetIdletime() → t` (uint64 milliseconds) — one-shot query.
  - `AddIdleWatch(interval: t) → id: u` — fires when the user has been idle `interval` ms (e.g. 30000 = 30 s; GNOME's own lock flow uses exactly this).
  - `AddUserActiveWatch() → id: u` — one-shot (no args), fires when the user becomes active again.
  - `RemoveWatch(id: u)`, `ResetIdletime()`.
  - Signal `WatchFired(id: u)` — delivers all watch notifications, identified by watch id.
- **Recommended design**: ship a small headless daemon that connects to the session bus (use the `zbus` crate in Rust), calls `AddIdleWatch(timeout)` and `AddUserActiveWatch()`, and on `WatchFired` for the idle watch, launches/shows your visualization in borderless fullscreen; on the user-active watch, hides/exits it. This mirrors how real GNOME idle/break apps (e.g. gnome-break-timer, which declares `--talk-name=org.gnome.Mutter.IdleMonitor`) integrate. It does *not* require being a real "screensaver" — GNOME on Wayland has no third-party `.saver`-style plugin system, so a standalone idle-triggered fullscreen app is the idiomatic approach.
- **Fullscreen**: use winit (which Bevy uses) with **`Fullscreen::Borderless`**, not `Exclusive`. The winit docs state verbatim: "Wayland: Does not support exclusive fullscreen mode and will no-op a request." (Older winit even panicked outright: Fyrox issue #22 records `thread 'main' panicked at 'Wayland doesn't support exclusive fullscreen'`.) Select the target monitor via `MonitorSelection`.
- **Multi-monitor & 5K**: enumerate monitors and either span one window per output or pick the primary. Be aware of a known winit/Wayland quirk where requesting a specific monitor for fullscreen can panic ("Unable to get monitor") [GitHub](https://github.com/bevyengine/bevy/issues/18556) — prefer `BorderlessFullscreen(Current)` or handle the monitor-enumeration edge cases. Respect the compositor's fractional-scaling: on Wayland you render at the surface's physical pixel size (true 5K) and should not assume logical = physical.
- **Idle-inhibit while running**: once your visualization is showing, you generally want to prevent the *real* screen blank/lock from kicking in on top of it. Hold an inhibit via the GNOME/freedesktop session API (`org.gnome.SessionManager.Inhibit`) or the portal — decide whether you want the visualization to *be* the screensaver (inhibit blanking) or to yield to the lock screen after a longer timeout.

### NVIDIA Blackwell (RTX 5070 Ti) on Ubuntu — critical hardware gotchas
- **You must use the open-kernel module driver.** Multiple field reports confirm the RTX 5070 Ti (Blackwell, `sm_120`) **only works with `nvidia-driver-*-open`**, not the fully proprietary module — NVIDIA states open modules are required for Blackwell. Use `nvidia-driver-570-open` at minimum (575/580+ preferred on 26.04). The proprietary package can fail to start GDM and leave `nvidia-smi` not detecting the GPU.
- **Explicit sync for flicker-free Wayland landed in NVIDIA 555.58.** Per 9to5Linux, "the biggest new feature of the NVIDIA 555.58 graphics driver [released June 27, 2024] is the explicit GPU sync support for Wayland via the `linux-drm-syncobj-v1` protocol," paired with compositor support in GNOME 46.1/Mutter, KDE Plasma 6.1, and Mesa 24.1. This eliminated the long-standing NVIDIA/Wayland flicker and frame-pacing problems. Ubuntu 26.04's GNOME + driver stack is well past this; just ensure a recent driver.
- **Enable `nvidia-drm.modeset=1`** (default-on with `fbdev` in 570.86.16+) for proper KMS/DRM handling and clean Wayland sessions.
- **wgpu + Wayland edge case**: there are reports of a `wp_linux_drm_syncobj_manager_v1 ... Surface already has a syncobj attached` error with wgpu+winit on some Wayland compositors; keep winit/wgpu reasonably current, where this is handled. If you hit it during development, an Xorg session [Rust Programming Language](https://users.rust-lang.org/t/wgpu-winit-surface-error-on-wayland/120804) is a temporary fallback, but target Wayland for production.
- **Sustained 5K performance**: 5K (5120×2880 ≈ 14.7M px, ~2.5× 1440p) is well within a 5070 Ti's reach for 2D/instanced generative art, but **bloom and other full-screen postprocessing passes scale with pixel count** — exactly why Observatory capped DPR at 2. Budget your passes, render the heavy generative/particle work via instancing and compute shaders, cap your frame rate sensibly (60 fps is plenty for ambient; uncapped wastes power/heat on a 24/7 process), and consider enabling VRR if your panel supports it (GNOME 46+ supports it as an experimental Mutter feature, requires Volta+ and driver ≥525).

### 5. Claude Code data fields available to drive visuals

You have two complementary data surfaces. **Drive *discrete animations* from the JSONL transcript, and *continuous visual state* from the OTel metrics.**

**A. JSONL transcript fields** (`~/.claude/projects/<encoded-path>/<session-id>.jsonl`, append-only, one JSON object per line):
- Top-level: `type` (`user`/`assistant`/`system`), `uuid`, `parentUuid` (builds the 
