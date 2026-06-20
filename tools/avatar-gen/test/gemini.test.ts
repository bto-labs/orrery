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
    expect(req.contents[0]).toEqual({
      inlineData: { data: Buffer.from("REF").toString("base64"), mimeType: "image/png" },
    }); // reference first
    expect(req.contents[1]).toEqual({ text: "make a sheet" }); // prompt last
    expect(req.config.imageConfig.aspectRatio).toBe("4:3");
    expect(req.config.imageConfig.imageSize).toBe("2K");
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
