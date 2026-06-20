# @orrery/avatar-gen

Stage 2 Subsystem A — per-repo character avatar generation pipeline for orrery.

Generates per-repo character-avatar sprites using base bot reference images and an image model.

## Usage

```bash
npm install
npm run derive    # derive avatar metadata from repo
npm run generate  # generate avatar sprites
npm run spike     # force-regenerate
```

## Assets

- `assets/bots/base/` — hand-authored base bot sprites (style anchors)
- `assets/agents/registry.json` — per-repo avatar registry

## Validation spike (one real generation)

This de-risks the novel image-gen step before trusting the prompt/slice layout.

1. Drop at least one base bot PNG into `../../assets/bots/base/`.
2. Export secrets (OpenBao-injected in deploy; for a local spike, source them):
   `GEMINI_API_KEY`, and for upload also `SEAWEEDFS_S3_ENDPOINT`,
   `SEAWEEDFS_S3_ACCESS_KEY_ID`, `SEAWEEDFS_S3_SECRET_ACCESS_KEY`,
   `GITEA_BASE_URL`, `GITEA_TOKEN`. Never paste secret values into the repo.
   Optionally override the default image model with `AVATAR_MODEL_ID`
   (defaults to `gemini-3-pro-image-preview`; verify this against current Gemini
   image-model availability before the first real run, as model names churn).
3. Layout-only validation (writes 5 frames to `spike-out/`, no upload):
   `GEMINI_API_KEY=… npx vitest run test/integration.live.test.ts`
4. Eyeball `spike-out/_sheet.png` and the 5 `spike-out/<pose>.png` frames:
   - exactly 5 cells, transparent gutters, same character in every cell?
   - if the slicer miscounts cells, tune the prompt's gutter wording or the
     `sliceGrid` `threshold`/`minRun`, **not** the pose contract.
5. Full end-to-end against the real repo + SeaweedFS:
   `npx tsx src/cli.ts generate ../../ --force`
   then inspect `assets/agents/registry.json` for the `frames` URIs.

If step 4 needs prompt changes, update `src/prompt.ts` and re-run — the unit
tests (synthetic fixture) still guarantee the slicer geometry.
