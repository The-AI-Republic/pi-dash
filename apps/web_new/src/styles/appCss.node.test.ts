// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stylesheet contract for the sidebar section titles (NEWPIDASH-4): the
// --border band is mid-tone in the high-contrast themes while captions
// stay --text-muted, so app.css drops the fill there. Parsed from the
// shipped file, not a copy (same approach as kit's tokens.test.ts).
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));

/** Comment-free stylesheet: prose may describe the rule, code carries it. */
function stylesheet(): string {
  const raw = readFileSync(join(here, "app.css"), "utf8");
  return raw.replace(/\/\*[\s\S]*?\*\//g, "");
}

/** Contents of every `@layer utilities { ... }` block, brace-matched. */
function utilitiesBlocks(css: string): string[] {
  const blocks: string[] = [];
  let from = 0;
  for (;;) {
    const at = css.indexOf("@layer utilities", from);
    if (at === -1) break;
    const open = css.indexOf("{", at);
    if (open === -1) break;
    let depth = 0;
    let close = -1;
    for (let i = open; i < css.length; i += 1) {
      if (css[i] === "{") depth += 1;
      if (css[i] === "}") {
        depth -= 1;
        if (depth === 0) {
          close = i;
          break;
        }
      }
    }
    if (close === -1) break;
    blocks.push(css.slice(open + 1, close));
    from = close + 1;
  }
  return blocks;
}

describe("app.css sidebar section headers", () => {
  it("drops the band in both high-contrast themes, and only there", () => {
    // --border is mid-tone under light-contrast/dark-contrast while the
    // captions stay --text-muted (≈1.35:1 / ≈1.55:1), so the headers
    // render without the fill there and keep their AA guarantee. The
    // rule must live in @layer utilities: bg-(--border) compiles there,
    // and a components-layer rule would lose by cascade order.
    const css = stylesheet();
    const marked = utilitiesBlocks(css).filter((rule) => rule.includes(".sidebar-section-header"));
    expect(marked.length).toBeGreaterThan(0);
    for (const rule of marked) {
      expect(rule).toMatch(/\[data-theme="/);
      expect(rule).toContain("background-color: transparent");
    }
    expect(marked.some((rule) => rule.includes("light-contrast"))).toBe(true);
    expect(marked.some((rule) => rule.includes("dark-contrast"))).toBe(true);
  });
});
