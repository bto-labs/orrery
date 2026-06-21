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
