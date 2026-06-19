# Handoff — Stage 1 complete: live RabbitMQ ingestion (v0.2.0)

## 1. Session Summary

Took orrery from the Stage-0 render POC to **Stage 1 fully shipped**: it now
visualizes **real, live Claude Code sessions across the homelab**, colored by
model, end-to-end.

Three things were done this session:
1. **§9 source-availability verification** — RabbitMQ **GO**, Mimir **dead** (no
   `claude_code_*` metrics exist), REST **no live feed**. This refuted the spec's
   "full three-source merge" and made Stage 1 RabbitMQ-only.
2. **Plan 1 (ingestion foundation)** — built additively under subagent-driven TDD:
   `src/ingest/` with a tokio runtime → bounded mpsc → a clock-free reducer
   (owns `HashMap<SessionId, AgentState>`, sole writer to `triple_buffer`) →
   dynamic per-session nuclei (spawn/fade/despawn) on the render side. The old
   `agent.rs`/`sync.rs` were removed and `main`/`visuals` flipped onto the seam in
   one lockstep commit. Verified live on Wayland at 5120×2160 on the RTX 5070 Ti.
3. **Plan 2 (live RabbitMQ)** — brainstormed → design spec → TDD plan → executed:
   a `lapin` hook source (`hook.#` → lifecycle/activity), a transcript source
   (`transcript.message` → per-session model, learn-once), live-by-default config,
   bounded AMQP queues. Live-verified against the real broker (rendered real
   session nuclei colored by model).

Every task got a two-stage (spec + quality) review; both plans got an opus
whole-branch review. The Plan-2 whole-branch review caught a real cross-cutting
bug (transcript `Summary` seeding permanent ghost nuclei) — fixed
(enrichment-only `Summary`). 26 unit tests, clippy clean throughout.

## 2. Pending Tasks

Nothing in flight. Deferred to a future **Plan 3** (all explicitly out of scope
of Stage 1, recorded in the design specs):
- Per-source health overlay (e.g. `RMQ ✓`), and all-sources-quiet → synthetic
  **auto-fallback** (so a broker outage shows the demo field instead of an empty one).
- `max_agents` **cap enforcement** (currently plumbed + logged; render shows all
  sessions — fine at homelab scale today).
- `agent_id`/`agent_type` subagent topology (currently one nucleus per `session_id`;
  the data is in `rawPayload`).
- Consumer-scaffold dedup (`consume_once` is near-identical in rabbitmq.rs/transcript.rs).
- Rapier physics / orbital bodies / inter-agent arcs; GNOME idle/D-Bus screensaver
  trigger. (Mimir + REST stay dropped unless the upstream pipeline changes.)

**Not yet pushed to origin** — the branch was merged to `main` *locally* only.

## 3. Session Metadata

- Date: 2026-06-19
- Repo: `/home/jay/dev/orrery` — remote `https://github.com/bto-labs/orrery.git`
- Branch: work done on `stage1-ingestion-foundation`, merged into `main` **locally**
  (no push). Version `v0.2.0`.
- Working tree: clean after the closeout commit + merge.
- Claude Code session ID: `ec02b129-bf34-46f0-8152-b9f146141b64`
- Machine: bto-storm (RTX 5070 Ti, driver 595.71.05, Ubuntu 26.04, GNOME Wayland)

## 4. Branch Information

`stage1-ingestion-foundation` held 18 commits (Plan 1 + the §9 verification doc +
Plan 2 + the closeout commit), all reviewed. Merged into `main` locally this
session. **Origin has NOT been updated** — per the user's standing "never push
without explicit instruction" rule. To publish: `git push origin main`.

## 5. File Inventory

- **New (Plan 1):** `src/ingest/{mod,model,reducer,synthetic}.rs`.
- **New (Plan 2):** `src/ingest/sources/{mod,rabbitmq,transcript}.rs`,
  `src/ingest/sources/fixtures/{hook_*.json, transcript_assistant.jsonl, SCHEMA.md}`.
- **Removed:** `src/agent.rs`, `src/sync.rs`, `docs/poc-screenshot.png`.
- **Modified:** `src/{main,visuals,diagnostics}.rs`, `Cargo.toml`/`Cargo.lock`
  (tokio, lapin, serde, futures-lite), `README.md`, `CHANGELOG.md`, `CLAUDE.md`,
  `PLAN.md`.
- **Docs added:** `docs/superpowers/specs/2026-06-17-stage1-source-verification.md`,
  `docs/superpowers/specs/2026-06-18-stage1-plan2-rabbitmq-design.md`,
  `docs/superpowers/plans/2026-06-18-stage1-plan2-rabbitmq.md`, this handoff.
- **Machine-side (not in repo):** orrery declares durable bounded queues
  `orrery.hook` + `orrery.transcript` on the `claude-events` broker (bound to
  `hook.#` / `transcript.message`, `x-message-ttl` 1h + `x-max-length` 10000).

## 6. Original Context

> "run the §9 verification and start plan 1 and delete that screenshot" →
> then "Author Plan 2 (RabbitMQ ingestion)" → "write the implementation plan" →
> "execute it with subagents" → "Close out + merge to main (local)".

Course-corrections during the session: chose the **additive, defer-the-swap**
execution order for Plan 1 (the plan's literal per-task deletions didn't compile
in order); for Plan 2 chose **live-by-default** + **RabbitMQ + transcript-model**
+ **show every homelab session** (no exclude filter) + **two independent consumers**.

## 7. Next Steps (priority order)

1. **Decide whether to `git push origin main`** (it's only local right now).
2. If continuing the visualization: **author Plan 3** (the deferred polish above —
   the highest-value items are the synthetic **auto-fallback** + per-source
   **health overlay**, since orrery is meant to run 24/7 and currently shows an
   empty field on a broker outage).
3. Optional: run it live during the day (`cargo run --release`, `RABBITMQ_URL`
   set) to see the field populated — this session verified at a quiet hour so
   only 1–2 sessions were active.
