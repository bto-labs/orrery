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
