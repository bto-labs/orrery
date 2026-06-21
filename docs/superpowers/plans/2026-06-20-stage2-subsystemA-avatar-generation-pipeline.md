# Stage 2 — Subsystem A: Avatar-Generation Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a standalone TypeScript worker that turns per-repo metadata + base bots into 5 cached, themed character-sprite frames (one per canonical pose) stored in SeaweedFS, keyed for regeneration on metadata change.

**Architecture:** A Node.js CLI worker, decoupled from the Rust renderer. It (1) auto-derives + merges per-repo metadata into a JSON registry, (2) builds a single contact-sheet prompt and calls the Gemini image model with the base bot(s) as reference images (the consistency trick — one generation, five poses), (3) slices the returned sheet into 5 labeled frames via alpha-gutter detection, (4) uploads frames to SeaweedFS (S3) under `repoKey/metadataHash/<pose>.png`, and (5) records frame URIs back into the registry. Every external boundary (Gemini, S3, gitea, git, clock) sits behind an interface so the pure logic — hashing, prompt assembly, slice geometry, cache decisions, the orchestrator — is unit-tested deterministically with fakes; the live model/S3 calls are exercised by gated integration tests + a one-shot validation spike.

**Tech Stack:** Node.js ≥20 LTS, TypeScript (strict), `@google/genai` (Gemini), `sharp` (image slicing), `@aws-sdk/client-s3` (SeaweedFS), `zod` (schema), `vitest` (tests), `tsx` (run/dev). Package manager: npm.

## Global Constraints

These apply to every task. Each task's requirements implicitly include this section.

- **Location:** the worker lives in `tools/avatar-gen/` inside the orrery repo (a self-contained npm package; it is **not** part of the Rust crate and is excluded from `cargo`).
- **Runtime:** Node.js ≥20 LTS, TypeScript `strict: true`. (Node chosen over Bun for `sharp` native-binding reliability and first-class `@google/genai`/`@aws-sdk` support; Bun is a viable swap if the user prefers.)
- **Secrets — never in code, never logged.** Read `GEMINI_API_KEY`, `SEAWEEDFS_S3_ACCESS_KEY_ID`, `SEAWEEDFS_S3_SECRET_ACCESS_KEY`, and `GITEA_TOKEN` from `process.env` only (OpenBao-injected at deploy, per `service-secrets-via-openbao`). Never hardcode, never print values; logs may reference key **names**, never their contents.
- **Model:** default `gemini-3-pro-image-preview` (Nano Banana Pro — strongest character consistency + legible on-sheet text; generation is one-time/cached so quality beats latency). It is a single config constant — **confirm the exact current id at build** (the `-preview` suffix and family names churn).
- **The 5-pose contract is fixed and shared with Subsystem B (the renderer):** `neutral, idle, active, attention, error`. Do not rename or reorder without updating the renderer contract (spec §6).
- **Output interface (what Subsystem B consumes):** `assets/agents/registry.json`, each repo entry carrying `metadataHash`, `generatedAt`, `modelId`, `spriteSheetUri`, and `frames: [{pose, uri}]` (exactly 5, in pose order). URIs are `s3://<bucket>/<repoKey>/<metadataHash>/<pose>.png`.
- **Cache key / idempotency:** `${repoKey}/${metadataHash}/${pose}.png` in the SeaweedFS bucket from `SEAWEEDFS_S3_BUCKET` (default `orrery-agent-sprites`). Same metadata → same hash → no regeneration. Generation is off the render path and one-time per repo.
- **Determinism for tests:** the wall clock is injected (`now: () => string`), mirroring the Rust reducer's "reads no wall clock" rule. No `Date.now()` in pure logic.
- **Green gates:** `npx tsc --noEmit` clean and `npx vitest run` green after every task. No `any` leaks on public function signatures.
- **Jay-owned inputs (do NOT block on these — fixtures stand in):** the 3–5 base bots land in `assets/bots/base/` at runtime; curated metadata fields (`themeHint`, `goals`, etc.) start blank and auto-derivation fills the rest. Tests use a tiny synthetic PNG fixture, never real bots.

---

## File Structure

```
tools/avatar-gen/
  package.json            # deps, scripts (test, build, derive, generate, spike)
  tsconfig.json           # strict TS, NodeNext
  vitest.config.ts        # node env
  .gitignore              # dist/, node_modules/, .env
  README.md               # how to run derive/generate/spike; env vars
  src/
    config.ts             # env-driven config + required-env assertions (no secret logging)
    poses.ts              # the 5 canonical poses + descriptions + order (Subsystem B contract)
    metadata/
      schema.ts           # RepoMetadata zod schema + type (spec §4)
      hash.ts             # metadataHash() — stable hash over identity+curated fields
      gitea.ts            # GiteaClient interface + HTTP impl (desc/topics/langs/created)
      derive.ts           # GitInspector interface + auto-derivation + curated-override merge
    registry.ts           # load/save assets/agents/registry.json; upsert; needsRegeneration()
    prompt.ts             # buildSheetPrompt(meta) — contact-sheet prompt (pure)
    gemini.ts             # ImageGenerator interface + GeminiImageGenerator impl
    slice/
      geometry.ts         # pure alpha-projection → ordered crop rects
      index.ts            # sliceSheet(): sharp raw → geometry → extract 5 frames
    storage.ts            # FrameStore interface + SeaweedFsStore (S3); key/uri builders
    pipeline.ts           # generateForRepo(): derive→hash→cache→generate→slice→upload→registry
    cli.ts                # `derive` / `generate` commands (node:util parseArgs)
    index.ts              # public exports
    __fixtures__/
      make-sheet.ts       # builds a synthetic 5-cell transparent sheet for tests
  test/
    *.test.ts             # vitest specs, colocated by module
assets/
  bots/base/README.md     # Jay drops 3–5 base bots here (png, transparent bg)
  bots/base/.gitkeep
  agents/registry.json    # the registry (seeded with an example entry)
```

---

## Task 1: Scaffold the worker package

**Files:**
- Create: `tools/avatar-gen/package.json`
- Create: `tools/avatar-gen/tsconfig.json`
- Create: `tools/avatar-gen/vitest.config.ts`
- Create: `tools/avatar-gen/.gitignore`
- Create: `tools/avatar-gen/README.md`
- Create: `tools/avatar-gen/src/index.ts`
- Create: `tools/avatar-gen/test/smoke.test.ts`
- Create: `assets/bots/base/README.md`
- Create: `assets/bots/base/.gitkeep`
- Create: `assets/agents/registry.json`

**Interfaces:**
- Consumes: nothing.
- Produces: a buildable, testable TS package; `assets/` layout for base bots + registry.

- [ ] **Step 1: Create `package.json`**

```json
{
  "name": "@orrery/avatar-gen",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "description": "Stage 2 Subsystem A — per-repo character avatar generation pipeline for orrery",
  "engines": { "node": ">=20" },
  "bin": { "avatar-gen": "./dist/cli.js" },
  "scripts": {
    "build": "tsc -p tsconfig.json",
    "typecheck": "tsc --noEmit",
    "test": "vitest run",
    "derive": "tsx src/cli.ts derive",
    "generate": "tsx src/cli.ts generate",
    "spike": "tsx src/cli.ts generate --force"
  },
  "dependencies": {
    "@aws-sdk/client-s3": "^3.700.0",
    "@google/genai": "^2.0.1",
    "sharp": "^0.34.0",
    "zod": "^3.24.0"
  },
  "devDependencies": {
    "@types/node": "^22.0.0",
    "tsx": "^4.19.0",
    "typescript": "^5.7.0",
    "vitest": "^3.0.0"
  }
}
```

- [ ] **Step 2: Create `tsconfig.json`, `vitest.config.ts`, `.gitignore`**

`tools/avatar-gen/tsconfig.json`:
```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "outDir": "dist",
    "rootDir": "src",
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "declaration": true,
    "sourceMap": true
  },
  "include": ["src"]
}
```

`tools/avatar-gen/vitest.config.ts`:
```ts
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["test/**/*.test.ts"],
  },
});
```

`tools/avatar-gen/.gitignore`:
```
node_modules/
dist/
.env
*.local.png
```

- [ ] **Step 3: Create stub `src/index.ts`, the assets layout, and the smoke test**

`tools/avatar-gen/src/index.ts`:
```ts
export const VERSION = "0.1.0";
```

`assets/bots/base/README.md`:
```markdown
# Base bots

Drop the 3–5 hand-authored base bot sprites here as transparent-background PNGs
(`bot-a.png`, `bot-b.png`, …). They are the **style anchor**: every generated
per-repo avatar is produced with one or more of these passed to the image model
as reference images, so the whole fleet shares a coherent look.

Guidance: full-body, front-facing, neutral pose, transparent background,
roughly square, ≥1024px. These are committed to the repo (they are inputs, not
generated output).
```

`assets/bots/base/.gitkeep`: (empty file)

`assets/agents/registry.json`:
```json
{
  "version": 1,
  "repos": {}
}
```

`tools/avatar-gen/test/smoke.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { VERSION } from "../src/index.js";

describe("package", () => {
  it("exposes a version", () => {
    expect(VERSION).toBe("0.1.0");
  });
});
```

- [ ] **Step 4: Install, typecheck, and run the smoke test**

Run:
```bash
cd tools/avatar-gen && npm install && npm run typecheck && npm test
```
Expected: install succeeds; `typecheck` exits 0; vitest reports `1 passed`.

- [ ] **Step 5: Commit**

```bash
git add tools/avatar-gen assets/bots/base assets/agents/registry.json
git commit -m "feat(avatar-gen): scaffold Subsystem A TS worker package + assets layout"
```

---

## Task 2: Canonical pose contract

**Files:**
- Create: `tools/avatar-gen/src/poses.ts`
- Create: `tools/avatar-gen/test/poses.test.ts`

**Interfaces:**
- Consumes: nothing.
- Produces: `POSE_ORDER: Pose[]` (length 5, reading order for the sheet & slicer), `type Pose = "neutral"|"idle"|"active"|"attention"|"error"`, `POSE_DESCRIPTIONS: Record<Pose,string>`.

- [ ] **Step 1: Write the failing test**

`tools/avatar-gen/test/poses.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { POSE_ORDER, POSE_DESCRIPTIONS } from "../src/poses.js";

describe("pose contract", () => {
  it("has exactly the 5 canonical poses in stable order", () => {
    expect(POSE_ORDER).toEqual(["neutral", "idle", "active", "attention", "error"]);
  });

  it("describes every pose", () => {
    for (const pose of POSE_ORDER) {
      expect(POSE_DESCRIPTIONS[pose]).toBeTruthy();
    }
    expect(Object.keys(POSE_DESCRIPTIONS)).toHaveLength(POSE_ORDER.length);
  });
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd tools/avatar-gen && npx vitest run test/poses.test.ts`
Expected: FAIL — cannot find module `../src/poses.js`.

- [ ] **Step 3: Implement `src/poses.ts`**

```ts
export const POSE_ORDER = ["neutral", "idle", "active", "attention", "error"] as const;

export type Pose = (typeof POSE_ORDER)[number];

/** Generation-side description of each pose. Maps 1:1 to renderer AgentState (spec §6). */
export const POSE_DESCRIPTIONS: Record<Pose, string> = {
  neutral: "standing calmly, facing forward, relaxed neutral expression (default / transition)",
  idle: "pensive and resting, looking slightly downward, contemplative (idle)",
  active: "energetically working, one hand raised mid-gesture, focused and busy (active)",
  attention: "alert and looking to one side, eyebrows raised as if called (needs attention)",
  error: "concerned, slight frown, shoulders lowered (error state)",
};
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd tools/avatar-gen && npx vitest run test/poses.test.ts`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add tools/avatar-gen/src/poses.ts tools/avatar-gen/test/poses.test.ts
git commit -m "feat(avatar-gen): define the 5 canonical pose contract"
```

---

## Task 3: Repo metadata schema

**Files:**
- Create: `tools/avatar-gen/src/metadata/schema.ts`
- Create: `tools/avatar-gen/test/schema.test.ts`

**Interfaces:**
- Consumes: nothing.
- Produces: `RepoMetadataSchema` (zod), `type RepoMetadata`, `type FrameEntry = {pose: Pose; uri: string}`. Identity + curated fields are required-with-defaults; system bookkeeping (`metadataHash`, `generatedAt`, `modelId`, `spriteSheetUri`, `frames`) is nullable/defaulted.

- [ ] **Step 1: Write the failing test**

`tools/avatar-gen/test/schema.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { RepoMetadataSchema } from "../src/metadata/schema.js";

const minimal = {
  repoKey: "gitea.bto.bar/BTO/orrery",
  displayName: "Orrery",
  owner: "BTO",
  isPersonal: false,
  primaryLanguage: "Rust",
};

describe("RepoMetadataSchema", () => {
  it("accepts a minimal object and applies defaults", () => {
    const m = RepoMetadataSchema.parse(minimal);
    expect(m.topics).toEqual([]);
    expect(m.themeHint).toBeNull();
    expect(m.frames).toEqual([]);
    expect(m.metadataHash).toBeNull();
  });

  it("rejects a missing required field", () => {
    const { repoKey: _omit, ...broken } = minimal;
    expect(() => RepoMetadataSchema.parse(broken)).toThrow();
  });

  it("preserves curated fields when present", () => {
    const m = RepoMetadataSchema.parse({ ...minimal, themeHint: "a jewel", iconMotifs: ["jewel"] });
    expect(m.themeHint).toBe("a jewel");
    expect(m.iconMotifs).toEqual(["jewel"]);
  });
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd tools/avatar-gen && npx vitest run test/schema.test.ts`
Expected: FAIL — cannot find module `../src/metadata/schema.js`.

- [ ] **Step 3: Implement `src/metadata/schema.ts`**

```ts
import { z } from "zod";
import { POSE_ORDER } from "../poses.js";

const PoseEnum = z.enum(POSE_ORDER);

export const FrameEntrySchema = z.object({
  pose: PoseEnum,
  uri: z.string(),
});
export type FrameEntry = z.infer<typeof FrameEntrySchema>;

export const RepoMetadataSchema = z.object({
  // --- identity / auto-derived ---
  repoKey: z.string().min(1),
  displayName: z.string().min(1),
  owner: z.string().min(1),
  isPersonal: z.boolean(),
  primaryLanguage: z.string().min(1),
  createdAt: z.string().nullable().default(null),
  ageDays: z.number().int().nonnegative().nullable().default(null),
  summary: z.string().default(""),
  category: z
    .enum(["infra", "app", "library", "bot", "visualization", "data", "docs", "unknown"])
    .default("unknown"),
  topics: z.array(z.string()).default([]),
  hosts: z.array(z.string()).default([]),

  // --- curated overrides (start blank; Jay edits) ---
  themeHint: z.string().nullable().default(null),
  goals: z.string().nullable().default(null),
  personalityTraits: z.array(z.string()).default([]),
  accentPalette: z.array(z.string()).default([]),
  baseBotPreference: z.string().nullable().default(null),
  iconMotifs: z.array(z.string()).default([]),

  // --- system / generation bookkeeping ---
  metadataHash: z.string().nullable().default(null),
  generatedAt: z.string().nullable().default(null),
  modelId: z.string().nullable().default(null),
  spriteSheetUri: z.string().nullable().default(null),
  frames: z.array(FrameEntrySchema).default([]),
});

export type RepoMetadata = z.infer<typeof RepoMetadataSchema>;
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd tools/avatar-gen && npx vitest run test/schema.test.ts`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add tools/avatar-gen/src/metadata/schema.ts tools/avatar-gen/test/schema.test.ts
git commit -m "feat(avatar-gen): RepoMetadata zod schema (spec §4)"
```

---

## Task 4: Metadata hash (cache-invalidation key)

**Files:**
- Create: `tools/avatar-gen/src/metadata/hash.ts`
- Create: `tools/avatar-gen/test/hash.test.ts`

**Interfaces:**
- Consumes: `RepoMetadata` (Task 3).
- Produces: `metadataHash(meta: RepoMetadata): string` — a 16-hex-char sha256 prefix over identity+curated fields only. Excludes volatile identity (`createdAt`, `ageDays`, `hosts`) and all system bookkeeping, so day-to-day age changes and prior generation results never trigger regeneration. Order-independent for array fields.

- [ ] **Step 1: Write the failing test**

`tools/avatar-gen/test/hash.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { RepoMetadataSchema } from "../src/metadata/schema.js";
import { metadataHash } from "../src/metadata/hash.js";

const base = RepoMetadataSchema.parse({
  repoKey: "gitea.bto.bar/BTO/orrery",
  displayName: "Orrery",
  owner: "BTO",
  isPersonal: false,
  primaryLanguage: "Rust",
  topics: ["bevy", "wgpu"],
  themeHint: "a jewel",
});

describe("metadataHash", () => {
  it("is stable for identical logical input", () => {
    const a = metadataHash(base);
    const b = metadataHash(RepoMetadataSchema.parse({ ...base }));
    expect(a).toBe(b);
    expect(a).toMatch(/^[0-9a-f]{16}$/);
  });

  it("is independent of array order", () => {
    const reordered = RepoMetadataSchema.parse({ ...base, topics: ["wgpu", "bevy"] });
    expect(metadataHash(reordered)).toBe(metadataHash(base));
  });

  it("changes when a curated field changes", () => {
    const themed = RepoMetadataSchema.parse({ ...base, themeHint: "a clockwork orrery" });
    expect(metadataHash(themed)).not.toBe(metadataHash(base));
  });

  it("ignores volatile + system fields (no spurious regeneration)", () => {
    const aged = RepoMetadataSchema.parse({
      ...base,
      ageDays: 999,
      createdAt: "2020-01-01",
      hosts: ["bto-storm"],
      generatedAt: "2026-06-20T00:00:00Z",
      modelId: "gemini-3-pro-image-preview",
      spriteSheetUri: "s3://x/y",
      frames: [{ pose: "idle", uri: "s3://x/idle.png" }],
    });
    expect(metadataHash(aged)).toBe(metadataHash(base));
  });
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd tools/avatar-gen && npx vitest run test/hash.test.ts`
Expected: FAIL — cannot find module `../src/metadata/hash.js`.

- [ ] **Step 3: Implement `src/metadata/hash.ts`**

```ts
import { createHash } from "node:crypto";
import type { RepoMetadata } from "./schema.js";

/** Deterministic JSON with recursively sorted object keys. */
function stableStringify(value: unknown): string {
  if (Array.isArray(value)) {
    return `[${value.map(stableStringify).join(",")}]`;
  }
  if (value && typeof value === "object") {
    const keys = Object.keys(value as Record<string, unknown>).sort();
    return `{${keys
      .map((k) => `${JSON.stringify(k)}:${stableStringify((value as Record<string, unknown>)[k])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value) ?? "null";
}

const sortedStrings = (xs: string[]): string[] => [...xs].sort();

/**
 * Hash of the fields that define the *character* — identity + curated theme.
 * Volatile identity (createdAt/ageDays/hosts) and all generation bookkeeping
 * are excluded so regeneration triggers only on a meaningful change.
 */
export function metadataHash(meta: RepoMetadata): string {
  const subject = {
    repoKey: meta.repoKey,
    displayName: meta.displayName,
    owner: meta.owner,
    isPersonal: meta.isPersonal,
    primaryLanguage: meta.primaryLanguage,
    summary: meta.summary,
    category: meta.category,
    topics: sortedStrings(meta.topics),
    themeHint: meta.themeHint,
    goals: meta.goals,
    personalityTraits: sortedStrings(meta.personalityTraits),
    accentPalette: meta.accentPalette, // ordered: palette order is meaningful
    baseBotPreference: meta.baseBotPreference,
    iconMotifs: sortedStrings(meta.iconMotifs),
  };
  return createHash("sha256").update(stableStringify(subject)).digest("hex").slice(0, 16);
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd tools/avatar-gen && npx vitest run test/hash.test.ts`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add tools/avatar-gen/src/metadata/hash.ts tools/avatar-gen/test/hash.test.ts
git commit -m "feat(avatar-gen): metadataHash cache-invalidation key"
```

---

## Task 5: Gitea client (auto-derivation source)

**Files:**
- Create: `tools/avatar-gen/src/metadata/gitea.ts`
- Create: `tools/avatar-gen/test/gitea.test.ts`

**Interfaces:**
- Consumes: nothing (takes an injected `fetch`-like function for testability).
- Produces: `interface GiteaClient { fetchRepo(owner, repo): Promise<GiteaRepoInfo> }`, `type GiteaRepoInfo = { description: string; topics: string[]; primaryLanguage: string | null; createdAt: string | null }`, and `class HttpGiteaClient implements GiteaClient`.

- [ ] **Step 1: Write the failing test**

`tools/avatar-gen/test/gitea.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { HttpGiteaClient } from "../src/metadata/gitea.js";

function fakeFetch(routes: Record<string, unknown>) {
  return async (url: string | URL): Promise<Response> => {
    const key = url.toString();
    const match = Object.keys(routes).find((r) => key.endsWith(r));
    if (!match) return new Response("not found", { status: 404 });
    return new Response(JSON.stringify(routes[match]), { status: 200 });
  };
}

describe("HttpGiteaClient", () => {
  it("merges repo, topics, and languages endpoints", async () => {
    const fetchImpl = fakeFetch({
      "/api/v1/repos/BTO/orrery": {
        description: "GPU ambient visualization",
        created_at: "2026-06-17T00:00:00Z",
      },
      "/api/v1/repos/BTO/orrery/topics": { topics: ["bevy", "wgpu"] },
      "/api/v1/repos/BTO/orrery/languages": { Rust: 90000, WGSL: 1200 },
    });
    const client = new HttpGiteaClient("https://gitea.bto.bar", "tok", fetchImpl);
    const info = await client.fetchRepo("BTO", "orrery");
    expect(info.description).toBe("GPU ambient visualization");
    expect(info.topics).toEqual(["bevy", "wgpu"]);
    expect(info.primaryLanguage).toBe("Rust"); // highest byte count
    expect(info.createdAt).toBe("2026-06-17T00:00:00Z");
  });

  it("degrades to empty info on a 404 repo", async () => {
    const client = new HttpGiteaClient("https://gitea.bto.bar", "tok", fakeFetch({}));
    const info = await client.fetchRepo("BTO", "ghost");
    expect(info.description).toBe("");
    expect(info.topics).toEqual([]);
    expect(info.primaryLanguage).toBeNull();
  });
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd tools/avatar-gen && npx vitest run test/gitea.test.ts`
Expected: FAIL — cannot find module `../src/metadata/gitea.js`.

- [ ] **Step 3: Implement `src/metadata/gitea.ts`**

```ts
export interface GiteaRepoInfo {
  description: string;
  topics: string[];
  primaryLanguage: string | null;
  createdAt: string | null;
}

export interface GiteaClient {
  fetchRepo(owner: string, repo: string): Promise<GiteaRepoInfo>;
}

type FetchLike = (url: string | URL, init?: RequestInit) => Promise<Response>;

const EMPTY: GiteaRepoInfo = { description: "", topics: [], primaryLanguage: null, createdAt: null };

export class HttpGiteaClient implements GiteaClient {
  constructor(
    private readonly baseUrl: string,
    private readonly token: string,
    private readonly fetchImpl: FetchLike = fetch,
  ) {}

  private async getJson<T>(path: string): Promise<T | null> {
    const res = await this.fetchImpl(`${this.baseUrl}${path}`, {
      headers: { Authorization: `token ${this.token}`, Accept: "application/json" },
    });
    if (!res.ok) return null;
    return (await res.json()) as T;
  }

  async fetchRepo(owner: string, repo: string): Promise<GiteaRepoInfo> {
    const base = `/api/v1/repos/${owner}/${repo}`;
    const repoJson = await this.getJson<{ description?: string; created_at?: string }>(base);
    if (!repoJson) return { ...EMPTY };

    const topicsJson = await this.getJson<{ topics?: string[] }>(`${base}/topics`);
    const langJson = await this.getJson<Record<string, number>>(`${base}/languages`);

    let primaryLanguage: string | null = null;
    if (langJson) {
      const sorted = Object.entries(langJson).sort((a, b) => b[1] - a[1]);
      primaryLanguage = sorted[0]?.[0] ?? null;
    }

    return {
      description: repoJson.description ?? "",
      topics: topicsJson?.topics ?? [],
      primaryLanguage,
      createdAt: repoJson.created_at ?? null,
    };
  }
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd tools/avatar-gen && npx vitest run test/gitea.test.ts`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add tools/avatar-gen/src/metadata/gitea.ts tools/avatar-gen/test/gitea.test.ts
git commit -m "feat(avatar-gen): gitea client for metadata auto-derivation"
```

---

## Task 6: Metadata auto-derivation + curated merge

**Files:**
- Create: `tools/avatar-gen/src/metadata/derive.ts`
- Create: `tools/avatar-gen/test/derive.test.ts`

**Interfaces:**
- Consumes: `GiteaClient` (Task 5), `RepoMetadataSchema`/`RepoMetadata` (Task 3).
- Produces: `interface GitInspector { remoteUrl(): Promise<string | null>; firstCommitDate(): Promise<string | null> }`, `parseRepoKey(remoteUrl): {host, owner, repo, repoKey} | null`, and `deriveMetadata(opts: { git: GitInspector; gitea: GiteaClient; curated?: Partial<RepoMetadata>; today: string }): Promise<RepoMetadata>`. Curated fields always win over derived defaults.

- [ ] **Step 1: Write the failing test**

`tools/avatar-gen/test/derive.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { parseRepoKey, deriveMetadata } from "../src/metadata/derive.js";
import type { GitInspector } from "../src/metadata/derive.js";
import type { GiteaClient } from "../src/metadata/gitea.js";

const gitea: GiteaClient = {
  async fetchRepo() {
    return {
      description: "GPU ambient visualization",
      topics: ["bevy", "wgpu"],
      primaryLanguage: "Rust",
      createdAt: "2026-06-17T00:00:00Z",
    };
  },
};

const git: GitInspector = {
  async remoteUrl() {
    return "git@gitea.bto.bar:BTO/orrery.git";
  },
  async firstCommitDate() {
    return "2026-06-17T00:00:00Z";
  },
};

describe("parseRepoKey", () => {
  it("parses ssh remotes", () => {
    expect(parseRepoKey("git@gitea.bto.bar:BTO/orrery.git")).toEqual({
      host: "gitea.bto.bar",
      owner: "BTO",
      repo: "orrery",
      repoKey: "gitea.bto.bar/BTO/orrery",
    });
  });
  it("parses https remotes", () => {
    expect(parseRepoKey("https://gitea.bto.bar/BTO/orrery.git")?.repoKey).toBe(
      "gitea.bto.bar/BTO/orrery",
    );
  });
});

describe("deriveMetadata", () => {
  it("derives identity + gitea fields and computes ageDays", async () => {
    const m = await deriveMetadata({ git, gitea, today: "2026-06-20T00:00:00Z" });
    expect(m.repoKey).toBe("gitea.bto.bar/BTO/orrery");
    expect(m.owner).toBe("BTO");
    expect(m.displayName).toBe("orrery");
    expect(m.primaryLanguage).toBe("Rust");
    expect(m.summary).toBe("GPU ambient visualization");
    expect(m.topics).toEqual(["bevy", "wgpu"]);
    expect(m.ageDays).toBe(3);
    expect(m.themeHint).toBeNull(); // curated, not auto-filled
  });

  it("lets curated overrides win", async () => {
    const m = await deriveMetadata({
      git,
      gitea,
      today: "2026-06-20T00:00:00Z",
      curated: { themeHint: "a jewel", displayName: "Orrery" },
    });
    expect(m.themeHint).toBe("a jewel");
    expect(m.displayName).toBe("Orrery");
  });
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd tools/avatar-gen && npx vitest run test/derive.test.ts`
Expected: FAIL — cannot find module `../src/metadata/derive.js`.

- [ ] **Step 3: Implement `src/metadata/derive.ts`**

```ts
import { RepoMetadataSchema, type RepoMetadata } from "./schema.js";
import type { GiteaClient } from "./gitea.js";

export interface GitInspector {
  remoteUrl(): Promise<string | null>;
  firstCommitDate(): Promise<string | null>;
}

export interface ParsedRepoKey {
  host: string;
  owner: string;
  repo: string;
  repoKey: string;
}

export function parseRepoKey(remoteUrl: string): ParsedRepoKey | null {
  // ssh: git@host:owner/repo(.git)   https: https://host/owner/repo(.git)
  const ssh = /^[\w.-]+@([\w.-]+):([\w.-]+)\/([\w.-]+?)(?:\.git)?$/.exec(remoteUrl);
  const https = /^https?:\/\/([\w.-]+)\/([\w.-]+)\/([\w.-]+?)(?:\.git)?$/.exec(remoteUrl);
  const m = ssh ?? https;
  if (!m) return null;
  const [, host, owner, repo] = m;
  return { host, owner, repo, repoKey: `${host}/${owner}/${repo}` };
}

function daysBetween(fromIso: string | null, todayIso: string): number | null {
  if (!fromIso) return null;
  const from = Date.parse(fromIso);
  const to = Date.parse(todayIso);
  if (Number.isNaN(from) || Number.isNaN(to)) return null;
  return Math.max(0, Math.floor((to - from) / 86_400_000));
}

export interface DeriveOptions {
  git: GitInspector;
  gitea: GiteaClient;
  today: string;
  curated?: Partial<RepoMetadata>;
}

export async function deriveMetadata(opts: DeriveOptions): Promise<RepoMetadata> {
  const remote = await opts.git.remoteUrl();
  const parsed = remote ? parseRepoKey(remote) : null;
  if (!parsed) throw new Error("could not derive repoKey from git remote");

  const info = await opts.gitea.fetchRepo(parsed.owner, parsed.repo);
  const createdAt = info.createdAt ?? (await opts.git.firstCommitDate());

  const derived = {
    repoKey: parsed.repoKey,
    displayName: parsed.repo,
    owner: parsed.owner,
    isPersonal: parsed.owner.toLowerCase() === "jay" || parsed.owner.toLowerCase() === "thiscode",
    primaryLanguage: info.primaryLanguage ?? "Unknown",
    createdAt,
    ageDays: daysBetween(createdAt, opts.today),
    summary: info.description,
    topics: info.topics,
  };

  // curated overrides win; drop undefined curated keys so they don't clobber derived values
  const curated = Object.fromEntries(
    Object.entries(opts.curated ?? {}).filter(([, v]) => v !== undefined),
  );

  return RepoMetadataSchema.parse({ ...derived, ...curated });
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd tools/avatar-gen && npx vitest run test/derive.test.ts`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add tools/avatar-gen/src/metadata/derive.ts tools/avatar-gen/test/derive.test.ts
git commit -m "feat(avatar-gen): metadata auto-derivation + curated-override merge"
```

---

## Task 7: Registry load/save + regeneration decision

**Files:**
- Create: `tools/avatar-gen/src/registry.ts`
- Create: `tools/avatar-gen/test/registry.test.ts`

**Interfaces:**
- Consumes: `RepoMetadata`/`RepoMetadataSchema` (Task 3), `metadataHash` (Task 4), `POSE_ORDER` (Task 2).
- Produces: `class Registry` with `static async load(path): Promise<Registry>`, `get(repoKey): RepoMetadata | undefined`, `upsert(meta): void`, `needsRegeneration(meta): boolean`, `async save(): Promise<void>`.

- [ ] **Step 1: Write the failing test**

`tools/avatar-gen/test/registry.test.ts`:
```ts
import { describe, it, expect, beforeEach } from "vitest";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Registry } from "../src/registry.js";
import { RepoMetadataSchema } from "../src/metadata/schema.js";
import { metadataHash } from "../src/metadata/hash.js";
import { POSE_ORDER } from "../src/poses.js";

const meta = RepoMetadataSchema.parse({
  repoKey: "gitea.bto.bar/BTO/orrery",
  displayName: "Orrery",
  owner: "BTO",
  isPersonal: false,
  primaryLanguage: "Rust",
  themeHint: "a jewel",
});

let path: string;
beforeEach(() => {
  const dir = mkdtempSync(join(tmpdir(), "reg-"));
  path = join(dir, "registry.json");
  writeFileSync(path, JSON.stringify({ version: 1, repos: {} }));
});

describe("Registry", () => {
  it("upserts and round-trips through disk", async () => {
    const reg = await Registry.load(path);
    reg.upsert(meta);
    await reg.save();
    const reloaded = await Registry.load(path);
    expect(reloaded.get(meta.repoKey)?.displayName).toBe("Orrery");
  });

  it("needsRegeneration is true for an unknown repo", async () => {
    const reg = await Registry.load(path);
    expect(reg.needsRegeneration(meta)).toBe(true);
  });

  it("needsRegeneration is false when hash matches and all 5 frames exist", async () => {
    const reg = await Registry.load(path);
    const done = RepoMetadataSchema.parse({
      ...meta,
      metadataHash: metadataHash(meta),
      frames: POSE_ORDER.map((pose) => ({ pose, uri: `s3://b/${pose}.png` })),
    });
    reg.upsert(done);
    expect(reg.needsRegeneration(meta)).toBe(false);
  });

  it("needsRegeneration is true when the hash drifts", async () => {
    const reg = await Registry.load(path);
    const done = RepoMetadataSchema.parse({
      ...meta,
      metadataHash: "deadbeefdeadbeef",
      frames: POSE_ORDER.map((pose) => ({ pose, uri: `s3://b/${pose}.png` })),
    });
    reg.upsert(done);
    expect(reg.needsRegeneration(meta)).toBe(true);
  });
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd tools/avatar-gen && npx vitest run test/registry.test.ts`
Expected: FAIL — cannot find module `../src/registry.js`.

- [ ] **Step 3: Implement `src/registry.ts`**

```ts
import { readFile, writeFile } from "node:fs/promises";
import { RepoMetadataSchema, type RepoMetadata } from "./metadata/schema.js";
import { metadataHash } from "./metadata/hash.js";
import { POSE_ORDER } from "./poses.js";

interface RegistryFile {
  version: number;
  repos: Record<string, unknown>;
}

export class Registry {
  private constructor(
    private readonly path: string,
    private readonly repos: Map<string, RepoMetadata>,
  ) {}

  static async load(path: string): Promise<Registry> {
    let parsed: RegistryFile = { version: 1, repos: {} };
    try {
      parsed = JSON.parse(await readFile(path, "utf8")) as RegistryFile;
    } catch {
      // missing/empty registry → start fresh
    }
    const repos = new Map<string, RepoMetadata>();
    for (const [key, value] of Object.entries(parsed.repos ?? {})) {
      repos.set(key, RepoMetadataSchema.parse(value));
    }
    return new Registry(path, repos);
  }

  get(repoKey: string): RepoMetadata | undefined {
    return this.repos.get(repoKey);
  }

  upsert(meta: RepoMetadata): void {
    this.repos.set(meta.repoKey, meta);
  }

  /** True when no cached generation matches the current metadata hash + full frame set. */
  needsRegeneration(meta: RepoMetadata): boolean {
    const existing = this.repos.get(meta.repoKey);
    if (!existing) return true;
    if (existing.metadataHash !== metadataHash(meta)) return true;
    const posesPresent = new Set(existing.frames.map((f) => f.pose));
    return !POSE_ORDER.every((p) => posesPresent.has(p));
  }

  async save(): Promise<void> {
    const repos: Record<string, RepoMetadata> = {};
    for (const [key, value] of [...this.repos.entries()].sort((a, b) => a[0].localeCompare(b[0]))) {
      repos[key] = value;
    }
    const out: RegistryFile = { version: 1, repos };
    await writeFile(this.path, `${JSON.stringify(out, null, 2)}\n`, "utf8");
  }
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd tools/avatar-gen && npx vitest run test/registry.test.ts`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add tools/avatar-gen/src/registry.ts tools/avatar-gen/test/registry.test.ts
git commit -m "feat(avatar-gen): registry load/save + regeneration decision"
```

---

## Task 8: Contact-sheet prompt assembly

**Files:**
- Create: `tools/avatar-gen/src/prompt.ts`
- Create: `tools/avatar-gen/test/prompt.test.ts`

**Interfaces:**
- Consumes: `RepoMetadata` (Task 3), `POSE_ORDER`/`POSE_DESCRIPTIONS` (Task 2).
- Produces: `buildSheetPrompt(meta: RepoMetadata): string` and `SHEET_LAYOUT = { rows: 2, cols: 3, cells: 5, aspectRatio: "4:3", imageSize: "2K" } as const`. Pure. The prompt encodes the consistency requirements (one image, same character, transparent background, wide gutters, labeled reading-order cells) that the slicer (Task 10) relies on.

- [ ] **Step 1: Write the failing test**

`tools/avatar-gen/test/prompt.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { buildSheetPrompt, SHEET_LAYOUT } from "../src/prompt.js";
import { POSE_ORDER } from "../src/poses.js";
import { RepoMetadataSchema } from "../src/metadata/schema.js";

const meta = RepoMetadataSchema.parse({
  repoKey: "gitea.bto.bar/BTO/orrery",
  displayName: "Orrery",
  owner: "BTO",
  isPersonal: false,
  primaryLanguage: "Rust",
  themeHint: "a jewel / clockwork orrery",
  iconMotifs: ["jewel", "orbit"],
  accentPalette: ["#6a5acd"],
});

describe("buildSheetPrompt", () => {
  it("names all 5 poses in reading order", () => {
    const p = buildSheetPrompt(meta);
    for (const pose of POSE_ORDER) expect(p).toContain(pose);
    expect(p.indexOf("neutral")).toBeLessThan(p.indexOf("error"));
  });

  it("encodes the theme and the slicing-critical layout constraints", () => {
    const p = buildSheetPrompt(meta);
    expect(p).toContain("a jewel / clockwork orrery");
    expect(p.toLowerCase()).toContain("transparent background");
    expect(p.toLowerCase()).toContain("single image");
    expect(p.toLowerCase()).toContain("same"); // same character across cells
    expect(p).toContain(`${SHEET_LAYOUT.cols}`);
  });

  it("falls back gracefully when curated theme is blank", () => {
    const blank = RepoMetadataSchema.parse({
      repoKey: "x/y/z",
      displayName: "Z",
      owner: "y",
      isPersonal: false,
      primaryLanguage: "Go",
    });
    const p = buildSheetPrompt(blank);
    expect(p).toContain("Go"); // language used as a theme hint fallback
    expect(p.length).toBeGreaterThan(100);
  });
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd tools/avatar-gen && npx vitest run test/prompt.test.ts`
Expected: FAIL — cannot find module `../src/prompt.js`.

- [ ] **Step 3: Implement `src/prompt.ts`**

```ts
import { POSE_ORDER, POSE_DESCRIPTIONS, type Pose } from "./poses.js";
import type { RepoMetadata } from "./metadata/schema.js";

export const SHEET_LAYOUT = {
  rows: 2,
  cols: 3, // 3 cells on the top row, 2 on the bottom (5 total, reading order)
  cells: 5,
  aspectRatio: "4:3",
  imageSize: "2K",
} as const;

function themeLine(meta: RepoMetadata): string {
  const theme = meta.themeHint ?? `a ${meta.primaryLanguage} project named "${meta.displayName}"`;
  const motifs = meta.iconMotifs.length ? ` Incorporate motif(s): ${meta.iconMotifs.join(", ")}.` : "";
  const palette = meta.accentPalette.length
    ? ` Accent colors: ${meta.accentPalette.join(", ")}.`
    : "";
  const personality = meta.personalityTraits.length
    ? ` Personality: ${meta.personalityTraits.join(", ")}.`
    : "";
  return `Theme: ${theme}.${motifs}${palette}${personality}`;
}

function poseLine(pose: Pose, index: number): string {
  return `${index + 1}. "${pose}" — ${POSE_DESCRIPTIONS[pose]}`;
}

export function buildSheetPrompt(meta: RepoMetadata): string {
  const poses = POSE_ORDER.map(poseLine).join("\n");
  return [
    "You are generating a sprite contact sheet for a single friendly robot character.",
    "Use the attached reference image(s) as the exact style anchor for the robot's body, proportions, and rendering style.",
    themeLine(meta),
    "",
    `Produce ONE single image: a contact sheet containing exactly ${SHEET_LAYOUT.cells} poses of THE SAME character.`,
    `Arrange them in reading order across ${SHEET_LAYOUT.rows} rows (${SHEET_LAYOUT.cols} cells on the top row, the remainder on the bottom).`,
    "Hard requirements (a downstream program slices this image):",
    "- Fully transparent background (alpha), no scenery, no shadow plate.",
    "- Wide, fully-transparent gutters separating every pose (both horizontally and vertically).",
    "- Each pose fully contained within its cell; the character is the same robot in every cell, only the pose/expression changes.",
    "- Consistent scale, lighting, and camera across all cells.",
    "The 5 poses, in order:",
    poses,
  ].join("\n");
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd tools/avatar-gen && npx vitest run test/prompt.test.ts`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add tools/avatar-gen/src/prompt.ts tools/avatar-gen/test/prompt.test.ts
git commit -m "feat(avatar-gen): contact-sheet prompt assembly (consistency trick)"
```

---

## Task 9: Slice geometry (pure alpha-projection)

**Files:**
- Create: `tools/avatar-gen/src/slice/geometry.ts`
- Create: `tools/avatar-gen/test/geometry.test.ts`

**Interfaces:**
- Consumes: nothing.
- Produces: `type Rect = { left: number; top: number; width: number; height: number }`, `detectBands(present: boolean[], minRun: number): Array<[number, number]>`, and `sliceGrid(alpha: Uint8Array, width: number, height: number, opts?: { threshold?: number; minRun?: number }): Rect[]` — returns content cells in reading order (top→bottom rows, left→right within a row).

- [ ] **Step 1: Write the failing test**

`tools/avatar-gen/test/geometry.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { detectBands, sliceGrid } from "../src/slice/geometry.js";

describe("detectBands", () => {
  it("finds runs of content separated by gutters", () => {
    // present = content. two blocks: [1..3] and [6..8]
    const present = [false, true, true, true, false, false, true, true, true, false];
    expect(detectBands(present, 1)).toEqual([
      [1, 3],
      [6, 8],
    ]);
  });

  it("ignores runs shorter than minRun (noise)", () => {
    const present = [true, false, false, false, false, true, true, true, true];
    expect(detectBands(present, 2)).toEqual([[5, 8]]);
  });
});

describe("sliceGrid", () => {
  it("splits a 2-row / 3-then-2 layout into 5 reading-order rects", () => {
    // Build a 14x9 alpha grid: rows 1..3 content, row 4 gutter, rows 5..7 content.
    // Top row content cols: 1..2, 5..6, 9..10  (3 cells)
    // Bottom row content cols: 1..2, 5..6       (2 cells)
    const w = 14;
    const h = 9;
    const alpha = new Uint8Array(w * h);
    const set = (x0: number, x1: number, y0: number, y1: number) => {
      for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) alpha[y * w + x] = 255;
    };
    set(1, 2, 1, 3);
    set(5, 6, 1, 3);
    set(9, 10, 1, 3);
    set(1, 2, 5, 7);
    set(5, 6, 5, 7);

    const rects = sliceGrid(alpha, w, h, { threshold: 128, minRun: 1 });
    expect(rects).toHaveLength(5);
    // reading order: first three from the top row, then two from the bottom
    expect(rects[0]).toEqual({ left: 1, top: 1, width: 2, height: 3 });
    expect(rects[2]).toEqual({ left: 9, top: 1, width: 2, height: 3 });
    expect(rects[3]).toEqual({ left: 1, top: 5, width: 2, height: 3 });
    expect(rects[4]).toEqual({ left: 5, top: 5, width: 2, height: 3 });
  });
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd tools/avatar-gen && npx vitest run test/geometry.test.ts`
Expected: FAIL — cannot find module `../src/slice/geometry.js`.

- [ ] **Step 3: Implement `src/slice/geometry.ts`**

```ts
export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Inclusive [start, end] index ranges of consecutive `true` runs at least `minRun` long. */
export function detectBands(present: boolean[], minRun: number): Array<[number, number]> {
  const bands: Array<[number, number]> = [];
  let start = -1;
  for (let i = 0; i <= present.length; i++) {
    const on = i < present.length && present[i];
    if (on && start === -1) start = i;
    if (!on && start !== -1) {
      if (i - start >= minRun) bands.push([start, i - 1]);
      start = -1;
    }
  }
  return bands;
}

export interface SliceOptions {
  threshold?: number;
  minRun?: number;
}

/**
 * Detect content cells by alpha projection. First split into horizontal rows
 * (rows with any opaque pixel), then split each row into columns. Returns rects
 * in reading order (top→bottom, left→right).
 */
export function sliceGrid(
  alpha: Uint8Array,
  width: number,
  height: number,
  opts: SliceOptions = {},
): Rect[] {
  const threshold = opts.threshold ?? 16;
  const minRun = opts.minRun ?? 2;
  const opaque = (x: number, y: number): boolean => (alpha[y * width + x] ?? 0) >= threshold;

  const rowPresent: boolean[] = [];
  for (let y = 0; y < height; y++) {
    let any = false;
    for (let x = 0; x < width && !any; x++) any = opaque(x, y);
    rowPresent.push(any);
  }

  const rects: Rect[] = [];
  for (const [y0, y1] of detectBands(rowPresent, minRun)) {
    const colPresent: boolean[] = [];
    for (let x = 0; x < width; x++) {
      let any = false;
      for (let y = y0; y <= y1 && !any; y++) any = opaque(x, y);
      colPresent.push(any);
    }
    for (const [x0, x1] of detectBands(colPresent, minRun)) {
      rects.push({ left: x0, top: y0, width: x1 - x0 + 1, height: y1 - y0 + 1 });
    }
  }
  return rects;
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd tools/avatar-gen && npx vitest run test/geometry.test.ts`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add tools/avatar-gen/src/slice/geometry.ts tools/avatar-gen/test/geometry.test.ts
git commit -m "feat(avatar-gen): pure alpha-projection slice geometry"
```

---

## Task 10: Sheet slicer (sharp) + synthetic-sheet fixture

**Files:**
- Create: `tools/avatar-gen/src/__fixtures__/make-sheet.ts`
- Create: `tools/avatar-gen/src/slice/index.ts`
- Create: `tools/avatar-gen/test/slice.test.ts`

**Interfaces:**
- Consumes: `sliceGrid`/`Rect` (Task 9), `Pose`/`POSE_ORDER` (Task 2).
- Produces: `makeSyntheticSheet(): Promise<Buffer>` (test fixture — a 5-cell transparent sheet with distinctly colored opaque blocks), and `sliceSheet(sheet: Buffer, poses: readonly Pose[]): Promise<Array<{ pose: Pose; png: Buffer }>>`. Throws if the detected cell count ≠ `poses.length`.

- [ ] **Step 1: Write the fixture and the failing test**

`tools/avatar-gen/src/__fixtures__/make-sheet.ts`:
```ts
import sharp from "sharp";

/**
 * Build a deterministic 2-row (3+2) contact sheet on a transparent background:
 * five opaque colored blocks separated by wide transparent gutters. Mirrors the
 * layout the prompt asks the model to produce, so the slicer can be tested
 * without a live generation.
 */
export async function makeSyntheticSheet(): Promise<Buffer> {
  const W = 600;
  const H = 400;
  const colors = [
    { r: 220, g: 40, b: 40 },
    { r: 40, g: 200, b: 60 },
    { r: 50, g: 80, b: 230 },
    { r: 230, g: 200, b: 30 },
    { r: 180, g: 40, b: 200 },
  ];
  // cell origins (3 on top row, 2 on bottom), each block 120x120 with gutters
  const cells = [
    { x: 40, y: 40 },
    { x: 240, y: 40 },
    { x: 440, y: 40 },
    { x: 40, y: 240 },
    { x: 240, y: 240 },
  ];
  const blocks = cells.map((c, i) => {
    const col = colors[i]!;
    return {
      input: {
        create: { width: 120, height: 120, channels: 4 as const, background: { ...col, alpha: 1 } },
      },
      left: c.x,
      top: c.y,
    };
  });
  return sharp({
    create: { width: W, height: H, channels: 4, background: { r: 0, g: 0, b: 0, alpha: 0 } },
  })
    .composite(blocks)
    .png()
    .toBuffer();
}
```

`tools/avatar-gen/test/slice.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import sharp from "sharp";
import { makeSyntheticSheet } from "../src/__fixtures__/make-sheet.js";
import { sliceSheet } from "../src/slice/index.js";
import { POSE_ORDER } from "../src/poses.js";

describe("sliceSheet", () => {
  it("splits a synthetic sheet into 5 pose-labeled frames", async () => {
    const sheet = await makeSyntheticSheet();
    const frames = await sliceSheet(sheet, POSE_ORDER);
    expect(frames.map((f) => f.pose)).toEqual([...POSE_ORDER]);
    // each frame should be ~120x120 (the block size), not the whole sheet
    const meta0 = await sharp(frames[0]!.png).metadata();
    expect(meta0.width).toBeGreaterThanOrEqual(110);
    expect(meta0.width).toBeLessThanOrEqual(130);
  });

  it("throws when the cell count does not match the pose count", async () => {
    const sheet = await makeSyntheticSheet();
    await expect(sliceSheet(sheet, ["neutral", "idle"])).rejects.toThrow(/expected 2/);
  });
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd tools/avatar-gen && npx vitest run test/slice.test.ts`
Expected: FAIL — cannot find module `../src/slice/index.js`.

- [ ] **Step 3: Implement `src/slice/index.ts`**

```ts
import sharp from "sharp";
import { sliceGrid } from "./geometry.js";
import type { Pose } from "../poses.js";

function alphaChannel(data: Buffer, width: number, height: number, channels: number): Uint8Array {
  const alpha = new Uint8Array(width * height);
  const aIndex = channels - 1; // RGBA → alpha is the last channel
  for (let i = 0; i < width * height; i++) {
    alpha[i] = channels >= 4 ? (data[i * channels + aIndex] ?? 255) : 255;
  }
  return alpha;
}

export async function sliceSheet(
  sheet: Buffer,
  poses: readonly Pose[],
): Promise<Array<{ pose: Pose; png: Buffer }>> {
  const { data, info } = await sharp(sheet).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
  const alpha = alphaChannel(data, info.width, info.height, info.channels);
  const rects = sliceGrid(alpha, info.width, info.height);

  if (rects.length !== poses.length) {
    throw new Error(`slice: expected ${poses.length} cells, detected ${rects.length}`);
  }

  const out: Array<{ pose: Pose; png: Buffer }> = [];
  for (let i = 0; i < rects.length; i++) {
    const png = await sharp(sheet).extract(rects[i]!).png().toBuffer();
    out.push({ pose: poses[i]!, png });
  }
  return out;
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd tools/avatar-gen && npx vitest run test/slice.test.ts`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add tools/avatar-gen/src/__fixtures__/make-sheet.ts tools/avatar-gen/src/slice/index.ts tools/avatar-gen/test/slice.test.ts
git commit -m "feat(avatar-gen): sharp sheet slicer + synthetic-sheet test fixture"
```

---

## Task 11: Gemini image-generator adapter

**Files:**
- Create: `tools/avatar-gen/src/gemini.ts`
- Create: `tools/avatar-gen/test/gemini.test.ts`

**Interfaces:**
- Consumes: nothing (the live `GoogleGenAI` client is injected).
- Produces: `interface ImageGenerator { generateSheet(input: { prompt: string; references: ReferenceImage[] }): Promise<Buffer> }`, `type ReferenceImage = { data: Buffer; mimeType: string }`, and `class GeminiImageGenerator implements ImageGenerator`. Reference image parts precede the text prompt in `contents`; `imageConfig` carries `aspectRatio`/`imageSize`.

- [ ] **Step 1: Write the failing test**

`tools/avatar-gen/test/gemini.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { GeminiImageGenerator } from "../src/gemini.js";

function fakeAi(record: { calls: unknown[] }, b64: string) {
  return {
    models: {
      async generateContent(req: unknown) {
        record.calls.push(req);
        return {
          candidates: [{ content: { parts: [{ inlineData: { data: b64, mimeType: "image/png" } }] } }],
        };
      },
    },
  };
}

describe("GeminiImageGenerator", () => {
  it("places references before the prompt and returns decoded bytes", async () => {
    const record = { calls: [] as unknown[] };
    const png = Buffer.from("PNGDATA");
    const gen = new GeminiImageGenerator(
      fakeAi(record, png.toString("base64")) as never,
      "gemini-3-pro-image-preview",
      { aspectRatio: "4:3", imageSize: "2K" },
    );

    const out = await gen.generateSheet({
      prompt: "make a sheet",
      references: [{ data: Buffer.from("REF"), mimeType: "image/png" }],
    });

    expect(out.toString()).toBe("PNGDATA");
    const req = record.calls[0] as {
      model: string;
      contents: Array<{ inlineData?: unknown; text?: string }>;
      config: { imageConfig: { aspectRatio: string; imageSize: string } };
    };
    expect(req.model).toBe("gemini-3-pro-image-preview");
    expect(req.contents[0]).toHaveProperty("inlineData"); // reference first
    expect(req.contents[1]).toEqual({ text: "make a sheet" }); // prompt last
    expect(req.config.imageConfig.aspectRatio).toBe("4:3");
  });

  it("throws when the response carries no image", async () => {
    const gen = new GeminiImageGenerator(
      { models: { async generateContent() { return { candidates: [{ content: { parts: [{ text: "nope" }] } }] }; } } } as never,
      "gemini-3-pro-image-preview",
      { aspectRatio: "4:3", imageSize: "2K" },
    );
    await expect(gen.generateSheet({ prompt: "x", references: [] })).rejects.toThrow(/no image/i);
  });
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd tools/avatar-gen && npx vitest run test/gemini.test.ts`
Expected: FAIL — cannot find module `../src/gemini.js`.

- [ ] **Step 3: Implement `src/gemini.ts`**

```ts
import type { GoogleGenAI } from "@google/genai";

export interface ReferenceImage {
  data: Buffer;
  mimeType: string;
}

export interface ImageGenerator {
  generateSheet(input: { prompt: string; references: ReferenceImage[] }): Promise<Buffer>;
}

export interface ImageConfig {
  aspectRatio: string;
  imageSize: string;
}

export class GeminiImageGenerator implements ImageGenerator {
  constructor(
    private readonly ai: Pick<GoogleGenAI, "models">,
    private readonly model: string,
    private readonly imageConfig: ImageConfig,
  ) {}

  async generateSheet(input: { prompt: string; references: ReferenceImage[] }): Promise<Buffer> {
    const contents = [
      ...input.references.map((r) => ({
        inlineData: { data: r.data.toString("base64"), mimeType: r.mimeType },
      })),
      { text: input.prompt },
    ];

    const response = await this.ai.models.generateContent({
      model: this.model,
      contents,
      config: { imageConfig: { aspectRatio: this.imageConfig.aspectRatio, imageSize: this.imageConfig.imageSize } },
    });

    const parts = response.candidates?.[0]?.content?.parts ?? [];
    for (const part of parts) {
      const data = part.inlineData?.data;
      if (data) return Buffer.from(data, "base64");
    }
    throw new Error("Gemini response contained no image data");
  }
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd tools/avatar-gen && npx vitest run test/gemini.test.ts`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add tools/avatar-gen/src/gemini.ts tools/avatar-gen/test/gemini.test.ts
git commit -m "feat(avatar-gen): Gemini image-generator adapter (references + imageConfig)"
```

---

## Task 12: SeaweedFS frame store (S3)

**Files:**
- Create: `tools/avatar-gen/src/storage.ts`
- Create: `tools/avatar-gen/test/storage.test.ts`

**Interfaces:**
- Consumes: nothing (an S3-`send`-shaped client is injected).
- Produces: `frameKey(repoKey, hash, pose): string`, `sheetKey(repoKey, hash): string`, `frameUri(bucket, key): string`, `interface FrameStore { put(key, body, contentType): Promise<string>; exists(key): Promise<boolean> }`, and `class SeaweedFsStore implements FrameStore`.

- [ ] **Step 1: Write the failing test**

`tools/avatar-gen/test/storage.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { frameKey, sheetKey, frameUri, SeaweedFsStore } from "../src/storage.js";

describe("key builders", () => {
  it("namespaces by repoKey/hash/pose", () => {
    expect(frameKey("gitea.bto.bar/BTO/orrery", "abc123", "idle")).toBe(
      "gitea.bto.bar/BTO/orrery/abc123/idle.png",
    );
    expect(sheetKey("gitea.bto.bar/BTO/orrery", "abc123")).toBe(
      "gitea.bto.bar/BTO/orrery/abc123/_sheet.png",
    );
    expect(frameUri("orrery-agent-sprites", "k/p.png")).toBe("s3://orrery-agent-sprites/k/p.png");
  });
});

describe("SeaweedFsStore", () => {
  it("puts an object and returns its s3 uri", async () => {
    const sent: Array<{ name: string; input: Record<string, unknown> }> = [];
    const fakeS3 = {
      async send(cmd: { constructor: { name: string }; input: Record<string, unknown> }) {
        sent.push({ name: cmd.constructor.name, input: cmd.input });
        return {};
      },
    };
    const store = new SeaweedFsStore(fakeS3 as never, "orrery-agent-sprites");
    const uri = await store.put("k/idle.png", Buffer.from("x"), "image/png");
    expect(uri).toBe("s3://orrery-agent-sprites/k/idle.png");
    expect(sent[0]?.name).toBe("PutObjectCommand");
    expect(sent[0]?.input.Bucket).toBe("orrery-agent-sprites");
    expect(sent[0]?.input.Key).toBe("k/idle.png");
  });
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd tools/avatar-gen && npx vitest run test/storage.test.ts`
Expected: FAIL — cannot find module `../src/storage.js`.

- [ ] **Step 3: Implement `src/storage.ts`**

```ts
import { S3Client, PutObjectCommand, HeadObjectCommand } from "@aws-sdk/client-s3";

export function frameKey(repoKey: string, hash: string, pose: string): string {
  return `${repoKey}/${hash}/${pose}.png`;
}

export function sheetKey(repoKey: string, hash: string): string {
  return `${repoKey}/${hash}/_sheet.png`;
}

export function frameUri(bucket: string, key: string): string {
  return `s3://${bucket}/${key}`;
}

export interface FrameStore {
  put(key: string, body: Buffer, contentType: string): Promise<string>;
  exists(key: string): Promise<boolean>;
}

/** S3 client surface we actually use — keeps the fake in tests tiny. */
type S3Like = Pick<S3Client, "send">;

export class SeaweedFsStore implements FrameStore {
  constructor(
    private readonly s3: S3Like,
    private readonly bucket: string,
  ) {}

  async put(key: string, body: Buffer, contentType: string): Promise<string> {
    await this.s3.send(
      new PutObjectCommand({ Bucket: this.bucket, Key: key, Body: body, ContentType: contentType }) as never,
    );
    return frameUri(this.bucket, key);
  }

  async exists(key: string): Promise<boolean> {
    try {
      await this.s3.send(new HeadObjectCommand({ Bucket: this.bucket, Key: key }) as never);
      return true;
    } catch {
      return false;
    }
  }
}

/** Build a path-style S3 client for SeaweedFS from env-injected config. */
export function makeSeaweedClient(cfg: {
  endpoint: string;
  region: string;
  accessKeyId: string;
  secretAccessKey: string;
}): S3Client {
  return new S3Client({
    endpoint: cfg.endpoint,
    region: cfg.region,
    forcePathStyle: true,
    credentials: { accessKeyId: cfg.accessKeyId, secretAccessKey: cfg.secretAccessKey },
  });
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd tools/avatar-gen && npx vitest run test/storage.test.ts`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add tools/avatar-gen/src/storage.ts tools/avatar-gen/test/storage.test.ts
git commit -m "feat(avatar-gen): SeaweedFS S3 frame store + path-style client"
```

---

## Task 13: Pipeline orchestrator

**Files:**
- Create: `tools/avatar-gen/src/pipeline.ts`
- Create: `tools/avatar-gen/test/pipeline.test.ts`

**Interfaces:**
- Consumes: `ImageGenerator`/`ReferenceImage` (Task 11), `FrameStore`/`frameKey`/`sheetKey` (Task 12), `Registry` (Task 7), `sliceSheet` (Task 10), `buildSheetPrompt` (Task 8), `metadataHash` (Task 4), `RepoMetadata` (Task 3), `POSE_ORDER` (Task 2).
- Produces: `interface PipelineDeps { generator: ImageGenerator; store: FrameStore; registry: Registry; baseBots: () => Promise<ReferenceImage[]>; now: () => string; modelId: string }` and `generateForRepo(meta: RepoMetadata, deps: PipelineDeps, opts?: { force?: boolean }): Promise<RepoMetadata>`. Cache-hit short-circuits (no generation). On a miss it generates → slices → uploads sheet + 5 frames → updates + saves the registry.

- [ ] **Step 1: Write the failing test**

`tools/avatar-gen/test/pipeline.test.ts`:
```ts
import { describe, it, expect, beforeEach } from "vitest";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Registry } from "../src/registry.js";
import { RepoMetadataSchema } from "../src/metadata/schema.js";
import { metadataHash } from "../src/metadata/hash.js";
import { POSE_ORDER } from "../src/poses.js";
import { makeSyntheticSheet } from "../src/__fixtures__/make-sheet.js";
import { generateForRepo, type PipelineDeps } from "../src/pipeline.js";
import type { ImageGenerator } from "../src/gemini.js";
import type { FrameStore } from "../src/storage.js";

const meta = RepoMetadataSchema.parse({
  repoKey: "gitea.bto.bar/BTO/orrery",
  displayName: "Orrery",
  owner: "BTO",
  isPersonal: false,
  primaryLanguage: "Rust",
  themeHint: "a jewel",
});

function fakeStore(): FrameStore & { puts: string[] } {
  const puts: string[] = [];
  return {
    puts,
    async put(key) {
      puts.push(key);
      return `s3://orrery-agent-sprites/${key}`;
    },
    async exists() {
      return false;
    },
  };
}

let regPath: string;
beforeEach(() => {
  const dir = mkdtempSync(join(tmpdir(), "pipe-"));
  regPath = join(dir, "registry.json");
  writeFileSync(regPath, JSON.stringify({ version: 1, repos: {} }));
});

async function deps(genCount: { n: number }, store: FrameStore): Promise<PipelineDeps> {
  const sheet = await makeSyntheticSheet();
  const generator: ImageGenerator = {
    async generateSheet() {
      genCount.n += 1;
      return sheet;
    },
  };
  return {
    generator,
    store,
    registry: await Registry.load(regPath),
    baseBots: async () => [{ data: Buffer.from("REF"), mimeType: "image/png" }],
    now: () => "2026-06-20T00:00:00Z",
    modelId: "gemini-3-pro-image-preview",
  };
}

describe("generateForRepo", () => {
  it("generates, slices, uploads sheet + 5 frames, and records the registry", async () => {
    const genCount = { n: 0 };
    const store = fakeStore();
    const d = await deps(genCount, store);
    const out = await generateForRepo(meta, d);

    expect(genCount.n).toBe(1);
    expect(out.frames.map((f) => f.pose)).toEqual([...POSE_ORDER]);
    expect(out.metadataHash).toBe(metadataHash(meta));
    expect(out.modelId).toBe("gemini-3-pro-image-preview");
    expect(store.puts).toHaveLength(6); // 1 sheet + 5 frames
    expect(store.puts).toContain("gitea.bto.bar/BTO/orrery/" + metadataHash(meta) + "/idle.png");

    const persisted = await Registry.load(regPath);
    expect(persisted.get(meta.repoKey)?.frames).toHaveLength(5);
  });

  it("short-circuits on a cache hit (no generation)", async () => {
    // pre-seed a complete generation
    const seed = await Registry.load(regPath);
    seed.upsert(
      RepoMetadataSchema.parse({
        ...meta,
        metadataHash: metadataHash(meta),
        frames: POSE_ORDER.map((pose) => ({ pose, uri: `s3://b/${pose}.png` })),
      }),
    );
    await seed.save();

    const genCount = { n: 0 };
    const store = fakeStore();
    const d = await deps(genCount, store);
    await generateForRepo(meta, d);
    expect(genCount.n).toBe(0);
    expect(store.puts).toHaveLength(0);
  });

  it("regenerates a cache hit when force is set", async () => {
    const seed = await Registry.load(regPath);
    seed.upsert(
      RepoMetadataSchema.parse({
        ...meta,
        metadataHash: metadataHash(meta),
        frames: POSE_ORDER.map((pose) => ({ pose, uri: `s3://b/${pose}.png` })),
      }),
    );
    await seed.save();

    const genCount = { n: 0 };
    const store = fakeStore();
    const d = await deps(genCount, store);
    await generateForRepo(meta, d, { force: true });
    expect(genCount.n).toBe(1);
  });
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd tools/avatar-gen && npx vitest run test/pipeline.test.ts`
Expected: FAIL — cannot find module `../src/pipeline.js`.

- [ ] **Step 3: Implement `src/pipeline.ts`**

```ts
import { metadataHash } from "./metadata/hash.js";
import { RepoMetadataSchema, type RepoMetadata } from "./metadata/schema.js";
import { buildSheetPrompt } from "./prompt.js";
import { sliceSheet } from "./slice/index.js";
import { POSE_ORDER } from "./poses.js";
import { frameKey, sheetKey, type FrameStore } from "./storage.js";
import type { ImageGenerator, ReferenceImage } from "./gemini.js";
import type { Registry } from "./registry.js";

export interface PipelineDeps {
  generator: ImageGenerator;
  store: FrameStore;
  registry: Registry;
  baseBots: () => Promise<ReferenceImage[]>;
  now: () => string;
  modelId: string;
}

export async function generateForRepo(
  meta: RepoMetadata,
  deps: PipelineDeps,
  opts: { force?: boolean } = {},
): Promise<RepoMetadata> {
  const hash = metadataHash(meta);

  if (!opts.force && !deps.registry.needsRegeneration(meta)) {
    return deps.registry.get(meta.repoKey) ?? meta;
  }

  const references = await deps.baseBots();
  const prompt = buildSheetPrompt(meta);
  const sheet = await deps.generator.generateSheet({ prompt, references });
  const frames = await sliceSheet(sheet, POSE_ORDER);

  const spriteSheetUri = await deps.store.put(sheetKey(meta.repoKey, hash), sheet, "image/png");
  const frameEntries = [];
  for (const frame of frames) {
    const uri = await deps.store.put(
      frameKey(meta.repoKey, hash, frame.pose),
      frame.png,
      "image/png",
    );
    frameEntries.push({ pose: frame.pose, uri });
  }

  const updated = RepoMetadataSchema.parse({
    ...meta,
    metadataHash: hash,
    generatedAt: deps.now(),
    modelId: deps.modelId,
    spriteSheetUri,
    frames: frameEntries,
  });

  deps.registry.upsert(updated);
  await deps.registry.save();
  return updated;
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd tools/avatar-gen && npx vitest run test/pipeline.test.ts`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add tools/avatar-gen/src/pipeline.ts tools/avatar-gen/test/pipeline.test.ts
git commit -m "feat(avatar-gen): pipeline orchestrator (derive→generate→slice→upload→registry)"
```

---

## Task 14: Config + CLI wiring

**Files:**
- Create: `tools/avatar-gen/src/config.ts`
- Create: `tools/avatar-gen/src/cli.ts`
- Create: `tools/avatar-gen/test/config.test.ts`
- Modify: `tools/avatar-gen/src/index.ts` (export the public surface)

**Interfaces:**
- Consumes: everything above.
- Produces: `loadConfig(env): Config` (validates + names missing required vars; never logs values), and a CLI with `derive <repoPath>` and `generate <repoPath> [--force] [--dry-run]`. `index.ts` re-exports `generateForRepo`, `deriveMetadata`, `Registry`, schema, and poses for Subsystem B / programmatic use.

- [ ] **Step 1: Write the failing test (config validation)**

`tools/avatar-gen/test/config.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { loadConfig } from "../src/config.js";

const full = {
  GEMINI_API_KEY: "g",
  SEAWEEDFS_S3_ENDPOINT: "https://s3.bto.bar:8333",
  SEAWEEDFS_S3_ACCESS_KEY_ID: "a",
  SEAWEEDFS_S3_SECRET_ACCESS_KEY: "s",
  GITEA_BASE_URL: "https://gitea.bto.bar",
  GITEA_TOKEN: "t",
};

describe("loadConfig", () => {
  it("applies defaults for bucket, region, and model", () => {
    const cfg = loadConfig(full);
    expect(cfg.s3Bucket).toBe("orrery-agent-sprites");
    expect(cfg.s3Region).toBe("us-east-1");
    expect(cfg.modelId).toBe("gemini-3-pro-image-preview");
  });

  it("lists every missing required var by NAME (never values)", () => {
    try {
      loadConfig({});
      throw new Error("should have thrown");
    } catch (e) {
      const msg = (e as Error).message;
      expect(msg).toContain("GEMINI_API_KEY");
      expect(msg).toContain("SEAWEEDFS_S3_ACCESS_KEY_ID");
      expect(msg).toContain("GITEA_TOKEN");
    }
  });

  it("honors model + bucket overrides", () => {
    const cfg = loadConfig({ ...full, AVATAR_MODEL_ID: "gemini-3.1-flash-image", SEAWEEDFS_S3_BUCKET: "custom" });
    expect(cfg.modelId).toBe("gemini-3.1-flash-image");
    expect(cfg.s3Bucket).toBe("custom");
  });
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd tools/avatar-gen && npx vitest run test/config.test.ts`
Expected: FAIL — cannot find module `../src/config.js`.

- [ ] **Step 3: Implement `src/config.ts`**

```ts
export interface Config {
  geminiApiKey: string;
  modelId: string;
  s3Endpoint: string;
  s3Region: string;
  s3Bucket: string;
  s3AccessKeyId: string;
  s3SecretAccessKey: string;
  giteaBaseUrl: string;
  giteaToken: string;
}

const REQUIRED = [
  "GEMINI_API_KEY",
  "SEAWEEDFS_S3_ENDPOINT",
  "SEAWEEDFS_S3_ACCESS_KEY_ID",
  "SEAWEEDFS_S3_SECRET_ACCESS_KEY",
  "GITEA_BASE_URL",
  "GITEA_TOKEN",
] as const;

export function loadConfig(env: Record<string, string | undefined>): Config {
  const missing = REQUIRED.filter((k) => !env[k]);
  if (missing.length) {
    // names only — never echo secret VALUES
    throw new Error(`missing required env: ${missing.join(", ")}`);
  }
  return {
    geminiApiKey: env.GEMINI_API_KEY!,
    modelId: env.AVATAR_MODEL_ID ?? "gemini-3-pro-image-preview",
    s3Endpoint: env.SEAWEEDFS_S3_ENDPOINT!,
    s3Region: env.SEAWEEDFS_S3_REGION ?? "us-east-1",
    s3Bucket: env.SEAWEEDFS_S3_BUCKET ?? "orrery-agent-sprites",
    s3AccessKeyId: env.SEAWEEDFS_S3_ACCESS_KEY_ID!,
    s3SecretAccessKey: env.SEAWEEDFS_S3_SECRET_ACCESS_KEY!,
    giteaBaseUrl: env.GITEA_BASE_URL!,
    giteaToken: env.GITEA_TOKEN!,
  };
}
```

- [ ] **Step 4: Run the config test to confirm it passes**

Run: `cd tools/avatar-gen && npx vitest run test/config.test.ts`
Expected: PASS (3 tests).

- [ ] **Step 5: Implement the git inspector, base-bot loader, and CLI**

`tools/avatar-gen/src/cli.ts`:
```ts
import { parseArgs } from "node:util";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { readdir, readFile } from "node:fs/promises";
import { join, extname, resolve } from "node:path";
import { GoogleGenAI } from "@google/genai";
import { loadConfig } from "./config.js";
import { HttpGiteaClient } from "./metadata/gitea.js";
import { deriveMetadata, type GitInspector } from "./metadata/derive.js";
import { Registry } from "./registry.js";
import { buildSheetPrompt, SHEET_LAYOUT } from "./prompt.js";
import { GeminiImageGenerator, type ReferenceImage } from "./gemini.js";
import { SeaweedFsStore, makeSeaweedClient } from "./storage.js";
import { generateForRepo } from "./pipeline.js";

const exec = promisify(execFile);

function gitInspector(repoPath: string): GitInspector {
  return {
    async remoteUrl() {
      try {
        const { stdout } = await exec("git", ["-C", repoPath, "remote", "get-url", "origin"]);
        return stdout.trim() || null;
      } catch {
        return null;
      }
    },
    async firstCommitDate() {
      try {
        const { stdout } = await exec("git", [
          "-C",
          repoPath,
          "log",
          "--reverse",
          "--format=%cI",
          "--max-parents=0",
        ]);
        return stdout.split("\n")[0]?.trim() || null;
      } catch {
        return null;
      }
    },
  };
}

async function loadBaseBots(dir: string, preference: string | null): Promise<ReferenceImage[]> {
  const files = (await readdir(dir)).filter((f) => extname(f).toLowerCase() === ".png").sort();
  if (files.length === 0) throw new Error(`no base bots found in ${dir} (add transparent PNGs)`);
  const chosen = preference && files.includes(preference) ? [preference] : [files[0]!];
  return Promise.all(
    chosen.map(async (f) => ({ data: await readFile(join(dir, f)), mimeType: "image/png" })),
  );
}

function nowIso(): string {
  return new Date().toISOString();
}

async function main(): Promise<void> {
  const { positionals, values } = parseArgs({
    allowPositionals: true,
    options: {
      force: { type: "boolean", default: false },
      "dry-run": { type: "boolean", default: false },
      registry: { type: "string", default: "assets/agents/registry.json" },
      "base-bots": { type: "string", default: "assets/bots/base" },
    },
  });

  const [command, repoPathArg] = positionals;
  const repoPath = resolve(repoPathArg ?? ".");

  if (command !== "derive" && command !== "generate") {
    console.error("usage: avatar-gen <derive|generate> <repoPath> [--force] [--dry-run]");
    process.exit(2);
  }

  const cfg = loadConfig(process.env);
  const gitea = new HttpGiteaClient(cfg.giteaBaseUrl, cfg.giteaToken);
  const registry = await Registry.load(values.registry);
  const existing0 = await peekCurated(registry, gitInspector(repoPath));
  const meta = await deriveMetadata({
    git: gitInspector(repoPath),
    gitea,
    today: nowIso(),
    curated: existing0,
  });

  if (command === "derive") {
    registry.upsert(meta);
    await registry.save();
    console.log(`derived ${meta.repoKey} (hash ${require("./metadata/hash.js")})`);
    console.log(JSON.stringify({ repoKey: meta.repoKey, displayName: meta.displayName, summary: meta.summary, topics: meta.topics }, null, 2));
    return;
  }

  if (values["dry-run"]) {
    console.log(`[dry-run] ${meta.repoKey}`);
    console.log(`[dry-run] model=${cfg.modelId} layout=${SHEET_LAYOUT.cols}x${SHEET_LAYOUT.rows} ${SHEET_LAYOUT.imageSize}`);
    console.log(buildSheetPrompt(meta));
    return;
  }

  const ai = new GoogleGenAI({ apiKey: cfg.geminiApiKey });
  const generator = new GeminiImageGenerator(ai, cfg.modelId, {
    aspectRatio: SHEET_LAYOUT.aspectRatio,
    imageSize: SHEET_LAYOUT.imageSize,
  });
  const s3 = makeSeaweedClient({
    endpoint: cfg.s3Endpoint,
    region: cfg.s3Region,
    accessKeyId: cfg.s3AccessKeyId,
    secretAccessKey: cfg.s3SecretAccessKey,
  });
  const store = new SeaweedFsStore(s3, cfg.s3Bucket);

  const result = await generateForRepo(
    meta,
    {
      generator,
      store,
      registry,
      baseBots: () => loadBaseBots(values["base-bots"], meta.baseBotPreference),
      now: nowIso,
      modelId: cfg.modelId,
    },
    { force: values.force },
  );
  console.log(`generated ${result.frames.length} frames for ${result.repoKey}`);
  for (const f of result.frames) console.log(`  ${f.pose} → ${f.uri}`);
}

/** Pull previously-curated overrides out of the registry so manual edits survive re-derivation. */
async function peekCurated(
  registry: Registry,
  git: GitInspector,
): Promise<Record<string, unknown> | undefined> {
  const remote = await git.remoteUrl();
  if (!remote) return undefined;
  const { parseRepoKey } = await import("./metadata/derive.js");
  const parsed = parseRepoKey(remote);
  if (!parsed) return undefined;
  const prev = registry.get(parsed.repoKey);
  if (!prev) return undefined;
  return {
    themeHint: prev.themeHint,
    goals: prev.goals,
    personalityTraits: prev.personalityTraits,
    accentPalette: prev.accentPalette,
    baseBotPreference: prev.baseBotPreference,
    iconMotifs: prev.iconMotifs,
    displayName: prev.displayName,
  };
}

main().catch((err: unknown) => {
  console.error(`avatar-gen failed: ${err instanceof Error ? err.message : String(err)}`);
  process.exit(1);
});
```

> **Note for the implementer:** remove the stray `require("./metadata/hash.js")` debug fragment in the `derive` branch — replace that log line with a real hash call:
> ```ts
> import { metadataHash } from "./metadata/hash.js";
> // ...
> console.log(`derived ${meta.repoKey} (hash ${metadataHash(meta)})`);
> ```
> (ESM + `strict` will reject `require`; this is called out so it isn't copied verbatim.)

`tools/avatar-gen/src/index.ts` (replace stub):
```ts
export const VERSION = "0.1.0";
export { generateForRepo, type PipelineDeps } from "./pipeline.js";
export { deriveMetadata, parseRepoKey, type GitInspector } from "./metadata/derive.js";
export { Registry } from "./registry.js";
export { RepoMetadataSchema, type RepoMetadata, type FrameEntry } from "./metadata/schema.js";
export { POSE_ORDER, POSE_DESCRIPTIONS, type Pose } from "./poses.js";
export { buildSheetPrompt, SHEET_LAYOUT } from "./prompt.js";
```

- [ ] **Step 6: Typecheck, run the full suite, and commit**

Run:
```bash
cd tools/avatar-gen && npm run typecheck && npm test
```
Expected: `typecheck` exits 0; vitest reports all suites passing (smoke, poses, schema, hash, gitea, derive, registry, prompt, geometry, slice, gemini, storage, pipeline, config).

```bash
git add tools/avatar-gen/src/config.ts tools/avatar-gen/src/cli.ts tools/avatar-gen/src/index.ts tools/avatar-gen/test/config.test.ts
git commit -m "feat(avatar-gen): config validation + CLI (derive/generate, dry-run, force)"
```

---

## Task 15: Live validation spike + gated integration test + docs

This is the de-risking step the spec demands (§5, §11: "validate the single-sheet→slice layout with a real generation before committing the prompt template"). It runs against the real Gemini API and (optionally) real SeaweedFS, gated behind env so CI without secrets stays green.

**Files:**
- Create: `tools/avatar-gen/test/integration.live.test.ts`
- Modify: `tools/avatar-gen/README.md` (the spike runbook)
- Modify: `tools/avatar-gen/vitest.config.ts` (exclude live tests from the default run)

**Interfaces:**
- Consumes: the full pipeline.
- Produces: a documented `npm run spike` flow that writes the 5 sliced frames locally for visual inspection, and a gated vitest integration test.

- [ ] **Step 1: Exclude live tests from the default run**

Edit `tools/avatar-gen/vitest.config.ts`:
```ts
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["test/**/*.test.ts"],
    exclude: ["test/**/*.live.test.ts", "node_modules/**"],
  },
});
```

- [ ] **Step 2: Write the gated live integration test**

`tools/avatar-gen/test/integration.live.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { writeFile, mkdir } from "node:fs/promises";
import { join } from "node:path";
import { GoogleGenAI } from "@google/genai";
import { buildSheetPrompt, SHEET_LAYOUT } from "../src/prompt.js";
import { GeminiImageGenerator } from "../src/gemini.js";
import { sliceSheet } from "../src/slice/index.js";
import { POSE_ORDER } from "../src/poses.js";
import { RepoMetadataSchema } from "../src/metadata/schema.js";
import { readFile, readdir } from "node:fs/promises";
import { extname } from "node:path";

const RUN = process.env.GEMINI_API_KEY ? describe : describe.skip;

RUN("live Gemini generation + slice (gated on GEMINI_API_KEY)", () => {
  it("produces a sheet that slices into exactly 5 frames", async () => {
    const meta = RepoMetadataSchema.parse({
      repoKey: "gitea.bto.bar/BTO/orrery",
      displayName: "Orrery",
      owner: "BTO",
      isPersonal: false,
      primaryLanguage: "Rust",
      themeHint: "a jewel / clockwork orrery",
      iconMotifs: ["jewel", "orbit"],
    });

    const dir = process.env.AVATAR_BASE_BOTS ?? "../../assets/bots/base";
    const botFiles = (await readdir(dir)).filter((f) => extname(f).toLowerCase() === ".png").sort();
    const references = botFiles.length
      ? [{ data: await readFile(join(dir, botFiles[0]!)), mimeType: "image/png" }]
      : [];

    const ai = new GoogleGenAI({ apiKey: process.env.GEMINI_API_KEY });
    const generator = new GeminiImageGenerator(ai, process.env.AVATAR_MODEL_ID ?? "gemini-3-pro-image-preview", {
      aspectRatio: SHEET_LAYOUT.aspectRatio,
      imageSize: SHEET_LAYOUT.imageSize,
    });

    const sheet = await generator.generateSheet({ prompt: buildSheetPrompt(meta), references });

    const out = join(process.cwd(), "spike-out");
    await mkdir(out, { recursive: true });
    await writeFile(join(out, "_sheet.png"), sheet);

    const frames = await sliceSheet(sheet, POSE_ORDER);
    for (const f of frames) await writeFile(join(out, `${f.pose}.png`), f.png);

    expect(frames).toHaveLength(POSE_ORDER.length);
  }, 120_000);
});
```

- [ ] **Step 3: Run the offline suite to confirm the live test is skipped**

Run: `cd tools/avatar-gen && npm test`
Expected: all unit suites pass; the live suite reports **skipped** (no `GEMINI_API_KEY` in the offline run).

- [ ] **Step 4: Document the spike runbook in the README**

Append to `tools/avatar-gen/README.md`:
```markdown
## Validation spike (one real generation)

This de-risks the novel image-gen step before trusting the prompt/slice layout.

1. Drop at least one base bot PNG into `../../assets/bots/base/`.
2. Export secrets (OpenBao-injected in deploy; for a local spike, source them):
   `GEMINI_API_KEY`, and for upload also `SEAWEEDFS_S3_ENDPOINT`,
   `SEAWEEDFS_S3_ACCESS_KEY_ID`, `SEAWEEDFS_S3_SECRET_ACCESS_KEY`,
   `GITEA_BASE_URL`, `GITEA_TOKEN`. Never paste secret values into the repo.
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
```

- [ ] **Step 5: Commit**

```bash
git add tools/avatar-gen/test/integration.live.test.ts tools/avatar-gen/vitest.config.ts tools/avatar-gen/README.md
git commit -m "test(avatar-gen): gated live generation spike + slice validation runbook"
```

---

## Self-Review

**1. Spec coverage (spec §4–§6, §11):**
- §4 metadata schema → Tasks 3 (schema), 5–6 (auto-derivation sources + merge), 7 (registry). ✅
- §4 `metadataHash` cache key → Task 4. ✅
- §5 TS worker + `@google/genai` → Tasks 11, 14. ✅
- §5 consistency trick (base bots as references, ONE sheet) → Tasks 8 (prompt), 11 (references-first contents). ✅
- §5 slice into 5 frames → Tasks 9–10. ✅
- §5 SeaweedFS caching keyed by `repoKey + metadataHash`, regenerate on change → Tasks 7, 12, 13. ✅
- §5 OpenBao key via env, never logged → Task 14 (config names-only error). ✅
- §6 the 5 canonical poses as a fixed contract → Task 2, threaded through 8/10/13. ✅
- §11 "validate single-sheet→slice with a real generation before committing the prompt" → Task 15. ✅
- §11 base bots delivered by Jay / blank curated fields → handled by fixtures + fallbacks (Tasks 8, 10, 14); not blocking. ✅

**2. Placeholder scan:** No "TBD/TODO/handle errors appropriately". The one deliberate teaching-note (the `require` fragment in Task 14) is explicitly flagged with the correct replacement so it is not copied verbatim. ✅

**3. Type consistency:** `RepoMetadata`, `Pose`/`POSE_ORDER`, `FrameStore.put/exists`, `ImageGenerator.generateSheet`, `frameKey/sheetKey/frameUri`, `Registry.{load,get,upsert,needsRegeneration,save}`, `metadataHash`, `buildSheetPrompt`/`SHEET_LAYOUT`, `generateForRepo`/`PipelineDeps` are defined once and consumed with matching signatures across tasks. `now`/`modelId` injection is consistent between Task 13 and Task 14. ✅

**Known follow-ups (out of scope for Subsystem A, recorded for Subsystem B / a phase-2):**
- The slicer assumes the model honors the transparent-gutter layout; Task 15 is the gate that proves it. If real output needs content-bbox trimming per cell, add a `sharp.trim()` pass inside `sliceSheet` (does not change interfaces).
- A `regenerate-all` batch command over every registry repo, and a renderer cache-miss placeholder, are Subsystem B concerns.
- Real base bots + curated `themeHint`/`goals` are Jay-owned inputs.

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-06-20-stage2-subsystemA-avatar-generation-pipeline.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration. REQUIRED SUB-SKILL: superpowers:subagent-driven-development.

**2. Inline Execution** — Execute tasks in this session using superpowers:executing-plans, batch execution with checkpoints.

**Which approach?**
