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
  const https = /^https?:\/\/([\w.-]+?)(?::\d+)?\/([\w.-]+)\/([\w.-]+?)(?:\.git)?$/.exec(remoteUrl);
  const m = ssh ?? https;
  if (!m) return null;
  const host = m[1]!;
  const owner = m[2]!;
  const repo = m[3]!;
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
