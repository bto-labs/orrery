Continuing orrery work from the previous session.
Working directory: /home/jay/dev/orrery.
Stage 1 is COMPLETE and live-verified: orrery visualizes real homelab Claude Code
sessions from RabbitMQ (hook.# + transcript.message), colored by model. The work
(Plan 1 foundation + Plan 2 live sources, v0.2.0) was merged into `main` LOCALLY
this session but is NOT pushed to origin.

## Read these in order BEFORE doing anything else:

1. docs/handoffs/2026-06-19-stage1-live-rabbitmq-ingestion.md
   — the handoff: what shipped, the deferred Plan-3 list (§2), branch/push state
     (§4: merged to main locally, NOT pushed), and the prioritized next steps (§7).
2. CHANGELOG.md ([orrery] v0.2.0) — the concrete what-shipped for Stage 1.
3. docs/superpowers/specs/2026-06-17-stage1-source-verification.md
   — the §9 findings that shaped the design: RabbitMQ-only (Mimir dead, REST no
     live feed), model comes only from transcript.message. Load-bearing for Plan 3.
4. CLAUDE.md — build/run (live-by-default; needs RABBITMQ_URL), the ingest
   architecture, and the hard-won gotchas (lapin 4.10 API, empty-AMQP-vhost→"/",
   enrichment-only Summary, bounded queues).

## Then: confirm with the user whether to `git push origin main` (Stage 1 is
local-only). If continuing the build, author Plan 3 — start with the highest-value
deferred items for a 24/7 display: the all-sources-quiet → synthetic auto-fallback
and the per-source health overlay (orrery currently shows an empty field on a
broker outage). Use the brainstorming → writing-plans → subagent-driven-development
flow, as Plan 2 did.
