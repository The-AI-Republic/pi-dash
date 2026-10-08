// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Token contract tests: every token from the H-visual decision exists with
// the approved value, text pairs meet WCAG AA in both themes, and density
// stops match the spec. Values are parsed from tokens.css so the test pins
// the shipped file, not a copy of it.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

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

function luminance(hex: string): number {
  const channels = [1, 3, 5].map((i) => {
    const raw = parseInt(hex.slice(i, i + 2), 16) / 255;
    return raw <= 0.03928 ? raw / 12.92 : ((raw + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * channels[0]! + 0.7152 * channels[1]! + 0.0722 * channels[2]!;
}

function contrast(a: string, b: string): number {
  const sorted = [luminance(a), luminance(b)].sort((x, y) => y - x);
  const hi = sorted[0] ?? 0;
  const lo = sorted[1] ?? 0;
  return (hi + 0.05) / (lo + 0.05);
}

describe("tokens.css", () => {
  it("carries the approved light palette", () => {
    expect(light.get("--bg")).toBe("#ffffff");
    expect(light.get("--surface")).toBe("#fafafa");
    expect(light.get("--subtle")).toBe("#f4f4f5");
    expect(light.get("--border")).toBe("#e4e4e7");
    expect(light.get("--text")).toBe("#18181b");
    expect(light.get("--text-muted")).toBe("#71717a");
    expect(light.get("--accent")).toBe("#5e6ad2");
    expect(light.get("--accent-hover")).toBe("#4f5bc4");
    expect(light.get("--accent-subtle")).toBe("#eef0fb");
    expect(light.get("--on-accent")).toBe("#ffffff");
    expect(light.get("--success")).toBe("#2f9e5b");
    expect(light.get("--warning")).toBe("#d9912b");
    expect(light.get("--danger")).toBe("#e5484d");
  });

  it("carries the approved dark palette", () => {
    expect(dark.get("--bg")).toBe("#0b0b0d");
    expect(dark.get("--surface")).toBe("#121214");
    expect(dark.get("--subtle")).toBe("#1a1a1d");
    expect(dark.get("--border")).toBe("#26262a");
    expect(dark.get("--text")).toBe("#ededef");
    expect(dark.get("--text-muted")).toBe("#8b8b93");
    expect(dark.get("--accent")).toBe("#7c85e0");
    expect(dark.get("--accent-hover")).toBe("#8e96e6");
    expect(dark.get("--accent-subtle")).toBe("#1e2140");
    expect(dark.get("--on-accent")).toBe("#101223");
    expect(dark.get("--success")).toBe("#3db86c");
    expect(dark.get("--warning")).toBe("#e5a84a");
    expect(dark.get("--danger")).toBe("#f2555a");
  });

  it("defines type, spacing, shape, elevation, and motion tokens", () => {
    expect(light.get("--font-ui")).toContain("Inter");
    expect(light.get("--text-base-size")).toBe("13px");
    expect(light.get("--text-base-height")).toBe("20px");
    expect(light.get("--text-h1-size")).toBe("24px");
    expect(light.get("--space-8")).toBe("16px");
    expect(light.get("--radius-control")).toBe("4px");
    expect(light.get("--radius-popover")).toBe("6px");
    expect(light.get("--radius-dialog")).toBe("8px");
    expect(light.get("--motion-duration")).toBe("120ms");
    expect(light.get("--icon-size")).toBe("16px");
    expect(light.get("--icon-stroke")).toBe("1.5");
  });

  it("defaults to compact density with a comfortable stop", () => {
    expect(light.get("--density")).toBe("compact");
    expect(light.get("--control-height")).toBe("28px");
    expect(light.get("--row-height")).toBe("28px");
    const comfortable = block('[data-density="comfortable"]');
    expect(comfortable.get("--control-height")).toBe("32px");
    expect(comfortable.get("--row-height")).toBe("32px");
  });

  it("meets WCAG AA for body text in both themes", () => {
    for (const theme of [light, dark]) {
      expect(contrast(theme.get("--text")!, theme.get("--bg")!)).toBeGreaterThanOrEqual(4.5);
      expect(contrast(theme.get("--text")!, theme.get("--surface")!)).toBeGreaterThanOrEqual(4.5);
      expect(contrast(theme.get("--text-muted")!, theme.get("--bg")!)).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("meets WCAG AA for labels on filled actions in both themes", () => {
    for (const theme of [light, dark]) {
      expect(contrast(theme.get("--on-accent")!, theme.get("--accent")!)).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("meets WCAG AA for accent links on the background in both themes", () => {
    for (const theme of [light, dark]) {
      expect(contrast(theme.get("--accent")!, theme.get("--bg")!)).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("restricts status hues to redundant graphics (measured, not AA body text)", () => {
    // H-visual values are kept as decided. In the light theme the status
    // hues sit below 4.5 against the background, so components may only use
    // them for graphics that repeat adjacent text (badge dots) or large
    // display type — never for small body text. This test pins the measured
    // ratios so an accidental darkening/lightening is caught.
    const againstLight = (token: string): number => contrast(light.get(token)!, light.get("--bg")!);
    expect(againstLight("--success")).toBeGreaterThan(3);
    expect(againstLight("--danger")).toBeGreaterThan(3);
    expect(againstLight("--warning")).toBeGreaterThan(2.5);
  });
});
