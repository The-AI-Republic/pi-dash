// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Type-preference effect (parity/specs/pages.md §9.1, PAGE-072): the
// stored compact/large choice must change the rendered type, not only a
// class name. jsdom resolves no cascade from app.css, so this reads the
// shipped stylesheet and asserts the winning declarations: each size
// redefines the same inherited type tokens, and compact is strictly
// smaller than the default/large choice for the body and every heading.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const CSS = readFileSync(fileURLToPath(new URL("../../../styles/app.css", import.meta.url)), "utf8");

/** The declarations of one component-layer rule, keyed by property. */
function declarations(selector: string): Map<string, string> {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = new RegExp(`(?:^|\\n)\\s*${escaped}\\s*\\{([^}]*)\\}`, "m").exec(CSS);
  if (match === null) throw new Error(`no rule for ${selector}`);
  const found = new Map<string, string>();
  for (const line of (match[1] ?? "").split(";")) {
    const [property, value] = line.split(":").map((part) => part.trim());
    if (property !== undefined && value !== undefined && property.startsWith("--")) found.set(property, value);
  }
  return found;
}

/** `rem` values only: every token these rules set is a rem length. */
function rem(value: string | undefined): number {
  if (value === undefined) throw new Error("missing declaration");
  const parsed = /^([\d.]+)rem$/.exec(value);
  if (parsed === null) throw new Error(`unexpected length ${value}`);
  return Number(parsed[1]);
}

const SIZES = ["small-font", "large-font"] as const;
const TOKENS = ["--text-base-size", "--text-h1-size", "--text-h2-size", "--text-h3-size"] as const;

describe("stored page type size reflows the page (PAGE-072)", () => {
  it.each(SIZES)("%s defines every shared type token", (size) => {
    const rules = declarations(`.text-size-${size}`);
    for (const token of TOKENS) expect(rules.has(token), token).toBe(true);
  });

  it("compact is strictly smaller than the default/large choice", () => {
    const small = declarations(".text-size-small-font");
    const large = declarations(".text-size-large-font");
    for (const token of TOKENS) {
      expect(rem(small.get(token)), token).toBeLessThan(rem(large.get(token)));
    }
  });

  it("the body reads the token both sizes set, rather than a fixed size", () => {
    const body = /\n\s*\.rich-text-content\s*\{([^}]*)\}/.exec(CSS);
    expect(body).not.toBeNull();
    expect(body?.[1]).toContain("font-size: var(--text-base-size)");
  });

  it.each(["serif", "sans-serif"])("the %s face reaches the title and the body", (face) => {
    expect(CSS).toContain(`.text-face-${face},`);
    expect(CSS).toContain(`.text-face-${face} .rich-text-content`);
  });
});
