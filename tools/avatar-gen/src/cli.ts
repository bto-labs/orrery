import { parseArgs } from "node:util";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { readdir, readFile } from "node:fs/promises";
import { join, extname, resolve } from "node:path";
import { GoogleGenAI } from "@google/genai";
import { loadConfig, loadGiteaConfig } from "./config.js";
import { HttpGiteaClient } from "./metadata/gitea.js";
import { deriveMetadata, parseRepoKey, type GitInspector } from "./metadata/derive.js";
import { metadataHash } from "./metadata/hash.js";
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

  // Gitea-subset config: all paths need Gitea creds; derive + dry-run need nothing else
  const giteaCfg = loadGiteaConfig(process.env);
  const gitea = new HttpGiteaClient(giteaCfg.giteaBaseUrl, giteaCfg.giteaToken);
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
    console.log(`derived ${meta.repoKey} (hash ${metadataHash(meta)})`);
    console.log(JSON.stringify({ repoKey: meta.repoKey, displayName: meta.displayName, summary: meta.summary, topics: meta.topics }, null, 2));
    return;
  }

  if (values["dry-run"]) {
    console.log(`[dry-run] ${meta.repoKey}`);
    console.log(`[dry-run] model=${giteaCfg.modelId} layout=${SHEET_LAYOUT.cols}x${SHEET_LAYOUT.rows} ${SHEET_LAYOUT.imageSize}`);
    console.log(buildSheetPrompt(meta));
    return;
  }

  // Full secrets only needed for real generate
  const cfg = loadConfig(process.env);
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
