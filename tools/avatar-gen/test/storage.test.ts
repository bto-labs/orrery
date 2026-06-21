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

describe("SeaweedFsStore.exists", () => {
  it("returns true when HeadObject resolves", async () => {
    const sent: Array<{ name: string; input: Record<string, unknown> }> = [];
    const fakeS3 = {
      async send(cmd: { constructor: { name: string }; input: Record<string, unknown> }) {
        sent.push({ name: cmd.constructor.name, input: cmd.input });
        return {};
      },
    };
    const store = new SeaweedFsStore(fakeS3 as never, "orrery-agent-sprites");
    const result = await store.exists("k/idle.png");
    expect(result).toBe(true);
    expect(sent[0]?.name).toBe("HeadObjectCommand");
    expect(sent[0]?.input.Key).toBe("k/idle.png");
  });

  it("returns false when HeadObject rejects with 404", async () => {
    const fakeS3 = {
      async send(_cmd: unknown) {
        throw { $metadata: { httpStatusCode: 404 }, name: "NotFound" };
      },
    };
    const store = new SeaweedFsStore(fakeS3 as never, "orrery-agent-sprites");
    const result = await store.exists("k/missing.png");
    expect(result).toBe(false);
  });

  it("rethrows when HeadObject rejects with a non-404 error", async () => {
    const infraError = { $metadata: { httpStatusCode: 500 }, name: "InternalError" };
    const fakeS3 = {
      async send(_cmd: unknown) {
        throw infraError;
      },
    };
    const store = new SeaweedFsStore(fakeS3 as never, "orrery-agent-sprites");
    await expect(store.exists("k/any.png")).rejects.toMatchObject(infraError);
  });
});
