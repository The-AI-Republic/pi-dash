// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Marketing styles must use tokens the kit actually defines (CLOUD-003/004, CLOUD-006–009).
// A custom property the kit never declares makes the utility a no-op: a
// missing spacing step collapses padding and gaps to zero, and a missing
// foreground token drops filled actions below AA. These tests read the
// stylesheet the kit ships, so a typo fails here instead of in a screenshot.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const kitTokens = readFileSync(resolve(process.cwd(), "../../packages/kit/src/tokens.css"), "utf8");
const declared = new Set([...kitTokens.matchAll(/(--[\w-]+)\s*:/g)].map((match) => match[1]!));

const marketingSources = [
  "MarketingFrame.tsx",
  "MarketingHome.tsx",
  "MarketingDocs.tsx",
  "MarketingDownloads.tsx",
  "MarketingPricing.tsx",
  "Backdrop.tsx",
];

function usedTokens(file: string): string[] {
  const source = readFileSync(new URL(`./components/${file}`, import.meta.url), "utf8");
  return [...source.matchAll(/\(--[\w-]+\)/g)].map((match) => match[0].slice(1, -1));
}

describe("marketing styles", () => {
  it.each(marketingSources)("only uses tokens the kit ships (%s)", (file) => {
    const missing = usedTokens(file).filter((token) => !declared.has(token));
    expect(missing).toEqual([]);
  });

  it("pins the spacing step the frame and cards rely on", () => {
    expect(usedTokens("MarketingFrame.tsx")).toContain("--space-6");
    expect(usedTokens("MarketingHome.tsx")).toContain("--space-6");
  });

  it("pins the filled-action foreground to the kit's on-accent token", () => {
    for (const file of marketingSources) {
      expect(usedTokens(file)).not.toContain("--accent-contrast");
    }
    expect(usedTokens("MarketingFrame.tsx")).toContain("--on-accent");
    expect(usedTokens("MarketingHome.tsx")).toContain("--on-accent");
    expect(usedTokens("MarketingDocs.tsx")).toContain("--on-accent");
    expect(usedTokens("MarketingDownloads.tsx")).toContain("--on-accent");
    expect(usedTokens("MarketingPricing.tsx")).toContain("--on-accent");
  });
});
