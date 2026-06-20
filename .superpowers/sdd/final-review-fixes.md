# Final Review Fixes

## Fix 1 — Typecheck test files

**Test type errors fixed:** 0 (none required).

The only issue was a structural one: `tsconfig.test.json` initially inherited
`rootDir: "src"` from the base config, which caused TS6059 errors for all 15
test files (not under rootDir). Fixed by overriding `rootDir: "."` in
`tsconfig.test.json` so both `src/` and `test/` are valid source roots.
No test file source code was touched.

Changes made:
- `tools/avatar-gen/tsconfig.test.json` — created; extends base tsconfig,
  overrides `rootDir: "."` and `noEmit: true`, includes `["src", "test"]`.
- `tools/avatar-gen/package.json` — updated `typecheck` script to:
  `tsc --noEmit && tsc -p tsconfig.test.json --noEmit`

## Fix 2 — Document realized frame-URI contract

Added **§3.1 Realized frame-URI contract (Subsystem A → B)** to
`docs/superpowers/specs/2026-06-19-stage2-character-avatars-design.md`,
inserted between §3 (Decomposition) and §4 (Project metadata schema).

Documents:
- URI shape: `s3://<bucket>/<repoKey>/<metadataHash>/<pose>.png`
- That `repoKey` is multi-segment and unencoded
  (e.g. `gitea.bto.bar/BTO/orrery`)
- Concrete example: `s3://orrery-agent-sprites/gitea.bto.bar/BTO/orrery/<metadataHash>/idle.png`
- Guidance: Subsystem B must reconstruct the key from discrete fields
  (`repoKey` + `metadataHash` + `pose`), not parse the opaque `uri` string.

## Final verification

```
npm run typecheck
> tsc --noEmit && tsc -p tsconfig.test.json --noEmit
(no output — both passes clean)

npm test
 Test Files  14 passed (14)
      Tests  47 passed (47)
 integration.live.test.ts: skipped (not in run output — gated live suite)
```
