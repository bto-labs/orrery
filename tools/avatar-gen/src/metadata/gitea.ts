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
