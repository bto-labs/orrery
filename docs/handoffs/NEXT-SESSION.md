Continuing orrery work from the previous session.
Working directory: /home/jay/dev/orrery.
Stage-0 POC is complete, verified on the RTX 5070 Ti, and pushed to `main`.
Stage 1 (real data ingestion) is fully scoped: spec + foundation plan approved.
Nothing is in flight; pick up at the Stage 1 verification gate.

## Read these in order BEFORE doing anything else:

1. docs/handoffs/2026-06-17-stage0-poc-and-stage1-scoping.md
   — the handoff: what shipped, the §9 verification still owed, and the
     prioritized next steps. Load-bearing: it says do the verification first.
2. docs/superpowers/specs/2026-06-17-stage1-ingestion-design.md
   — the approved Stage 1 design; §9 is the source-availability gate that blocks
     the live-source code.
3. docs/superpowers/plans/2026-06-17-stage1-ingestion-foundation.md
   — the 6-task TDD plan for the foundation (tokio/reducer/dynamic-nuclei on
     synthetic); executable now, independent of the verification.
4. CLAUDE.md
   — build deps, the Wayland run command, Bevy 0.18 gotchas, and the
     GNOME/Mutter FPS-measurement caveat.

## Then: run the §9 source-availability verification — confirm (a) the
claude-events RabbitMQ exchange/routing-keys/envelope + creds reachable from
bto-storm (cross-check the mesh-six claude-events-consumer schema), (b) whether
Mimir actually exports `claude_code_*` with a `session.id` label (needs Grafana
creds — the MCP was unauthorized last session), and (c) a usable REST endpoint.
That unblocks authoring Plan 2. In parallel you can start executing Plan 1
(foundation on synthetic) via the subagent-driven-development skill.
