// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Chart color contract tests (parity/specs/analytics.md §10,
// NEWFRONT-732). The charts.ts values are parsed against tokens.css
// so the test pins the shipped stylesheet, not a copy of it; the
// stylesheet stays the single source and this module the mirror.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import { CHART_CATEGORICAL_DARK, CHART_CATEGORICAL_LIGHT, chartSchemeColors } from "./charts";

// The suite runs with the package directory as cwd
// (pnpm --filter @pidash/kit test), so the shipped file is read directly.
const css = readFileSync(join(process.cwd(), "src", "tokens.css"), "utf8");

function block(selector: string): Map<string, string> {
  const start = css.indexOf(selector);
  if (start === -1) throw new Error(`missing block ${selector}`);
  const open = css.indexOf("{", start);
  let depth = 0;
  let end = open;
  for (let i = open; i < css.length; i += 1) {
    if (css[i] === "{") depth += 1;
    if (css[i] === "}") {
      depth -= 1;
      if (depth === 0) {
        end = i;
        break;
      }
    }
  }
  const vars = new Map<string, string>();
  for (const match of css.slice(open + 1, end).matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) {
    vars.set(match[1]!, match[2]!.trim());
  }
  return vars;
}

const light = block(":root");
const dark = block('[data-theme="dark"]');

describe("chart color mirror", () => {
  it("mirrors the ten oracle-pinned light slots", () => {
    expect(CHART_CATEGORICAL_LIGHT).toHaveLength(10);
    expect([...CHART_CATEGORICAL_LIGHT]).toEqual([
      "#6172e8",
      "#8b6edb",
      "#e05f99",
      "#29a383",
      "#cb8a37",
      "#3aa7c1",
      "#f1b24a",
      "#e84855",
      "#50c799",
      "#b35f9e",
    ]);
    CHART_CATEGORICAL_LIGHT.forEach((slot, index) => {
      expect(light.get(`--chart-${index + 1}`)).toBe(slot);
    });
  });

  it("mirrors the ten dark slots", () => {
    expect(CHART_CATEGORICAL_DARK).toHaveLength(10);
    CHART_CATEGORICAL_DARK.forEach((slot, index) => {
      expect(dark.get(`--chart-${index + 1}`)).toBe(slot);
    });
    // Dark slots differ from their light twins (brighter for dark surfaces).
    expect(CHART_CATEGORICAL_DARK).not.toEqual([...CHART_CATEGORICAL_LIGHT]);
  });

  it("resolves the light scheme from the light tokens", () => {
    const colors = chartSchemeColors("light");
    expect(colors.accent).toBe(light.get("--accent"));
    expect(colors.grid).toBe(light.get("--border"));
    expect(colors.tick).toBe(light.get("--text-muted"));
    expect(colors.categorical).toBe(CHART_CATEGORICAL_LIGHT);
  });

  it("resolves the dark scheme from the dark tokens", () => {
    const colors = chartSchemeColors("dark");
    expect(colors.accent).toBe(dark.get("--accent"));
    expect(colors.grid).toBe(dark.get("--border"));
    expect(colors.tick).toBe(dark.get("--text-muted"));
    expect(colors.categorical).toBe(CHART_CATEGORICAL_DARK);
  });
});
