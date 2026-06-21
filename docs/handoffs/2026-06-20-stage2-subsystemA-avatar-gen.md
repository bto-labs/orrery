# Handoff — Stage 2 Subsystem A complete: avatar-generation pipeline (avatar-gen v0.1.0)

## 1. Session Summary

Built **Stage 2 Subsystem A** — the per-repo character-avatar **generation
pipeline** — end to end, and merged it to `main` locally. This is the first half
of the Stage-2 pivot: agents will render as representational **bot characters**
themed per repo (not abstract glowing dots). Subsystem A produces the cached
sprite frames that the future renderer (Subsystem B) will consume.

The work this session:
1. **Authored the Subsystem-A implementation plan** (writing-plans) from the
   approved Stage-2 design spec — 15 bite-sized TDD tasks, every external boundary
   behind a fakeable interface. Research first nailed the load-bearing unknowns
   (the `@google/genai` image API, the current Nano Banana model family, the
   homelab SeaweedFS/OpenBao conventions) so no API was fabricated.
2. **Executed all 15 tasks** via subagent-driven-development: a fresh implementer
   per task + a two-stage (spec + quality) review after each, fix-and-re-review on
   every Important finding, then a final whole-branch review on opus.
3. **Closed out** (this doc, README/CHANGELOG/CLAUDE.md/PLAN sync) and **merged to
   `main` locally** (no push, per the standing guardrail).

Result: `tools/avatar-gen/` — a standalone Node 20+/TS worker (decoupled from the
Rust crate). Flow: per-repo metadata (auto-derived from git remote + gitea, curated
overrides win) → one Gemini contact-sheet (base bots as reference images, 5 poses in
a single generation — the consistency trick) → `sharp` alpha-gutter slice into 5
pose-labeled frames → SeaweedFS upload → `assets/agents/registry.json`. 47 unit
tests; `tsc --noEmit` clean over both `src` and `test`; the live Gemini call is a
gated spike (`test/integration.live.test.ts`, runs only with `GEMINI_API_KEY`).

Review outcome: **no Critical findings, no correctness bugs.** Every task-level
Important finding was fixed + re-reviewed (notably: the planted `require()` ESM bug;
`https` repoKey port-stripping for the cache key; registry `load()` never throwing on
a bad entry; `exists()` discriminating 404 from real infra errors; the cache-hit
invariant throwing instead of returning un-enriched metadata; **derive/dry-run not
requiring Gemini/S3 secrets**). The opus whole-branch review's two items —
typechecking the test files, and documenting the realized frame-URI contract (§3.1)
for Subsystem B — are both fixed. Per-task commits + every finding are recorded in
`.superpowers/sdd/progress.md` (gitignored scratch).

## 2. Pending Tasks

**Jay-owned inputs (block real avatar output, not the code):**
- Deliver 3–5 base bots → `assets/bots/base/` (transparent-bg PNGs; the style anchor).
- Fill curated metadata per repo (`themeHint`, `goals`, `iconMotifs`, `accentPalette`,
  `personalityTraits`) — auto-derivation seeds the rest.
- Confirm the current Gemini image model id before the first real spike (`AVATAR_MODEL_ID`
  overrides the `gemini-3-pro-image-preview` default; names churn).
- Run the validation spike (`tools/avatar-gen/README.md`) to confirm the real
  single-sheet→slice layout before trusting it.

**Next build phase:**
- **Subsystem B — the renderer rework** (Rust/Bevy): consume the cached frames, render
  character sprites with expression-by-state, Rapier/force-field motion, orbital
  activity icons from `hook.*`, inter-agent arcs, the human-pairing. Not started.

**Deferred Minors** (non-blocking, logged in `.superpowers/sdd/progress.md` for
Subsystem-B time): node-domexception low-sev transitive vuln; gitea sub-requests
sequential; registry load conflates missing-vs-corrupt (no warn log); `ageDays`
churns `registry.json` on each derive; slicer noise-path only validated by the spike.

## 3. Session Metadata

- Date: 2026-06-20
- Repo: `/home/jay/dev/orrery` — remote `https://github.com/bto-labs/orrery.git`
- Branch: work on `stage2-subsystemA-avatar-gen`, merged into `main` **locally** (no push).
- Working tree after closeout: clean.
- Versions: `avatar-gen` v0.1.0 (new); orrery Rust crate stays **v0.2.0** (renderer
  unchanged — Subsystem A is a separate package and the app behaves identically).
- Claude Code session ID: `ec02b129-bf34-46f0-8152-b9f146141b64`
- Machine: bto-storm (RTX 5070 Ti, Ubuntu 26.04, GNOME Wayland)

## 4. Branch Information

`stage2-subsystemA-avatar-gen` held the 15-task build (each task: implementer commit
+ review, plus fix commits for Important findings), the final-review fixes, and the
closeout commit. Merged into `main` **locally** with `--no-ff` this session. **Origin
has NOT been updated** — per the standing "never push without explicit instruction"
rule. To publish: `git push origin main`.

## 5. File Inventory

- **New (worker):** `tools/avatar-gen/` — `package.json`, `tsconfig.json`,
  `tsconfig.test.json`, `vitest.config.ts`, `.gitignore`, `README.md`,
  `src/{config,poses,prompt,gemini,storage,registry,pipeline,cli,index}.ts`,
  `src/metadata/{schema,hash,gitea,derive}.ts`, `src/slice/{geometry,index}.ts`,
  `src/__fixtures__/make-sheet.ts`, `test/*.test.ts` (incl. gated
  `integration.live.test.ts`).
- **New (assets):** `assets/bots/base/{README.md,.gitkeep}`, `assets/agents/registry.json`.
- **New (docs):** `docs/superpowers/plans/2026-06-20-stage2-subsystemA-avatar-generation-pipeline.md`,
  this handoff.
- **Modified:** `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `PLAN.md`,
  `docs/superpowers/specs/2026-06-19-stage2-character-avatars-design.md` (+§3.1).

## 6. Original Context

> Resuming after compaction: Stage 2 (character avatars) was DESIGNED (approved spec
> `docs/superpowers/specs/2026-06-19-stage2-character-avatars-design.md`); the next
> step was turning it into implementation plans. Directive: "Invoke writing-plans for
> Stage-2 Subsystem A — the avatar-generation pipeline … Build A first." Then: execute
> it subagent-driven; on completion, "Closeout + local merge (no push)."

Guardrails honored throughout: secrets env-only (never logged); nothing pushed; the
Stage-2 spec/plan commits are local. AFFiNE is parked in the bto-devops repo (separate
session) — nothing to do here.

## 7. Next Steps (priority order)

1. **Jay: drop 3–5 base bots into `assets/bots/base/`** + confirm the Gemini model id,
   then run the spike (`tools/avatar-gen/README.md`) to validate the real
   single-sheet→slice layout. This is the de-risking gate before trusting generation.
2. **Author the Subsystem B plan** (writing-plans) — the renderer rework that consumes
   the cached frames (character sprites, expression-by-state, Rapier/force-field motion,
   orbital `hook.*` icons, inter-agent arcs, human pairing). It locates frames via the
   discrete `repoKey`+`metadataHash`+`pose` fields (spec §3.1), not by parsing the `uri`.
3. **Decide on push:** `main` is one Stage-2 merge ahead of origin (plus the earlier
   unpushed Stage-2 spec commit) — push only when Jay says so.
