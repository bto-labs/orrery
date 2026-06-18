# Handoff — Stage-0 POC complete + Stage 1 scoped

## 1. Session Summary

Built the **Stage-0 proof of concept** for orrery (GPU ambient visualization of
Claude Code agent activity) end-to-end on the target hardware, then **scoped
Stage 1** (real data ingestion) through to an approved spec and implementation
plan.

- POC: Bevy 0.18.1 binary rendering glowing nuclei + mote field with HDR+bloom at
  native 5120×2160 on Wayland/Vulkan; lock-free `triple_buffer` seam from a
  synthetic producer thread to the render loop; framerate-independent smoothing;
  runtime controls. All 7 acceptance criteria met (keybindings verified with real
  `ydotool` keystrokes; visuals confirmed via framebuffer screenshot).
- Verified render stack: **Vulkan**, **RTX 5070 Ti**, driver **595.71.05**, true
  **5120×2160**, no `linux-drm-syncobj` errors. GPU has ~8× headroom (≈2 ms frame
  work, ~66% util at 20k motes + bloom). Key finding: wall-clock FPS is
  unreliable on the live GNOME/Mutter desktop — documented with the telemetry
  workaround in `POC_RESULTS.md`.
- Stage 1 decisions: **full three-source merge** (RabbitMQ + REST + Mimir) ·
  **one nucleus per active session, homelab-wide** (spawn/idle/despawn) ·
  **synthetic retained** as `--synthetic` dev mode + auto-fallback. Architecture:
  tokio runtime → per-source tasks → bounded mpsc → single reducer that owns the
  model and is the only writer to the unchanged `triple_buffer`.

## 2. Pending Tasks

- **§9 source-availability verification (do this FIRST — it blocks Plan 2):**
  1. RabbitMQ: reachable URL + creds from bto-storm; exact exchange, routing-key
     pattern, envelope JSON (cross-check the mesh-six `claude-events-consumer`).
  2. Mimir: confirm `claude_code_*` series exist with `usePrometheusNaming` +
     cumulative temporality and a `session.id` label; query endpoint + tenant.
     (Grafana MCP was unauthorized this session — needs creds.)
  3. REST: find an endpoint with useful per-session state/metadata, else scope it
     to reconcile/no-op.
- **Author Plan 2** (the three live sources) after verification captures real
  payloads — its parsers were deliberately NOT written without real schemas.
- **Execute Plan 1** (foundation: tokio/reducer/dynamic-nuclei on synthetic).
- Open question: whether to commit the 5.5 MB `docs/poc-screenshot.png` long-term
  (committed this session as the POC visual record).

## 3. Session Metadata

- Date: 2026-06-17
- Repo: `/home/jay/dev/orrery` — remote `https://github.com/bto-labs/orrery.git`
- Branch: `main` (pushed at end of session per explicit "wrap and push")
- Working tree: clean after the closeout commit
- Claude Code session ID: `66b08a04-b3cd-40f5-b499-40f820fa2d22`
- Machine: bto-storm (RTX 5070 Ti, driver 595.71.05)

## 4. Branch Information

Work landed directly on `main` (user authorized push). No feature branch, no PR.

## 5. File Inventory

Created: `Cargo.toml`, `Cargo.lock`, `src/main.rs`, `src/agent.rs`, `src/sync.rs`,
`src/visuals.rs`, `src/diagnostics.rs`, `POC_RESULTS.md`, `CHANGELOG.md`,
`CLAUDE.md`, `docs/poc-screenshot.png`,
`docs/superpowers/specs/2026-06-17-stage1-ingestion-design.md`,
`docs/superpowers/plans/2026-06-17-stage1-ingestion-foundation.md`,
`docs/handoffs/2026-06-17-stage0-poc-and-stage1-scoping.md`,
`docs/handoffs/NEXT-SESSION.md`.
Modified: `README.md` (placeholder → full docs).
Machine-side (not in repo): installed `libwayland-dev libxkbcommon-dev
libasound2-dev libudev-dev`, mise-managed `rust@1.96.0`, and `ydotool` (its
systemd **user** service auto-starts `ydotoold` on login — `systemctl --user
disable ydotool` to undo).

## 6. Original Context

> Bootstrap `orrery`, a GPU-accelerated ambient visualization of live Claude
> Code agent activity (`docs/orrery-poc-prompt.md` + `PLAN.md`). Stage 0: prove
> Bevy/wgpu holds ~60 fps borderless-fullscreen at native 5K on Wayland with
> bloom, and prove the `triple_buffer` producer→render seam with smoothing, using
> synthetic data. Then: verify keybindings, then scope Stage 1.

User course-corrections during the session: install Rust via mise (done); chose
the full three-source merge, session-as-agent identity homelab-wide, and synthetic
fallback for Stage 1; "wrap and push".

## 7. Next Steps (priority order)

1. **Run the §9 verification** (RabbitMQ schema/creds, Mimir `claude_code_*`,
   REST endpoint) — unblocks everything live.
2. **Execute Plan 1** (foundation on synthetic) via subagent-driven-development —
   it's independent of the verification and de-risks the architecture.
3. **Author + execute Plan 2** (the three sources) once §9 is done.
