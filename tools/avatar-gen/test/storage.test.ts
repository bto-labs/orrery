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
