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
    const repos = new Map<string, RepoMetadata>();
    try {
      const parsed = JSON.parse(await readFile(path, "utf8")) as RegistryFile;
      for (const [key, value] of Object.entries(parsed.repos ?? {})) {
        repos.set(key, RepoMetadataSchema.parse(value));
      }
    } catch {
      // missing, unparseable, or schema-invalid registry → start fresh
      repos.clear();
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
