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
