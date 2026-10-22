// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the kit token check: undefined `-(--token)` utilities and
// fallback-less `var(--token)` references fail with file:line, while defined
// tokens, var() fallbacks, block comments, and `@theme` tokens pass.
import { afterAll, describe, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { isCliEntry, main, runTokenCheck } from "./check-tokens.mjs";

const script = join(dirname(fileURLToPath(import.meta.url)), "check-tokens.mjs");
const tempRoots = [];

function makeRepo(files) {
  const root = mkdtempSync(join(tmpdir(), "token-check-"));
  tempRoots.push(root);
  for (const [rel, content] of Object.entries(files)) {
    const full = join(root, rel);
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, content);
  }
  return root;
}

afterAll(() => {
  for (const root of tempRoots) rmSync(root, { recursive: true, force: true });
});

const TOKENS_CSS = [
  ":root {",
  "  --space-4: 8px;",
  "  --accent: #5e6ad2;",
  "  /* --ghost: 1px; */",
  "}",
  '[data-theme="dark"] {',
  "  --accent: #7c85e0;",
  "  --dark-only: #000000;",
  "}",
  "",
].join("\n");

const KIT_CSS = ["@theme {", "  --animate-shimmer: shimmer 1.6s linear infinite;", "}", ""].join("\n");

function options(root) {
  return {
    root,
    trees: ["apps/web_new/src", "packages/kit/src"],
    tokensCss: join(root, "packages", "kit", "src", "tokens.css"),
    kitCss: join(root, "packages", "kit", "src", "kit.css"),
  };
}

describe("runTokenCheck", () => {
  it("flags undefined utilities and fallback-less vars with file:line", () => {
    const root = makeRepo({
      "packages/kit/src/tokens.css": TOKENS_CSS,
      "packages/kit/src/kit.css": KIT_CSS,
      "apps/web_new/src/ok.tsx": [
        'const a = "p-(--space-4)";',
        'const b = "w-(--animate-shimmer)";',
        'const c = "color: var(--accent);";',
        'const d = "color: var(--nope, #fff);";',
        "/* p-(--space-5) var(--nope) */",
        'const e = "color: var( --accent );";',
        "",
      ].join("\n"),
      "apps/web_new/src/bad.tsx": [
        'const a = "gap-(--space-5)";',
        'const b = "color: var(--nope);";',
        'const c = "color: var(--dark-only);";',
        'const d = "px-(--space-9) ml-(--space-9)";',
        'const e = "p-(--ghost)";',
        "",
      ].join("\n"),
    });

    const failures = runTokenCheck(options(root));

    // ok.tsx is fully clean: defined utility, @theme token, defined var(),
    // var() with fallback, and tokens inside a block comment all pass.
    expect(failures.every((failure) => failure.includes("bad.tsx"))).toBe(true);
    expect(failures).toHaveLength(5);
    expect(failures[0]).toContain("bad.tsx:1");
    expect(failures[0]).toContain("--space-5");
    expect(failures[1]).toContain("bad.tsx:2");
    expect(failures[1]).toContain("--nope");
    // --dark-only exists only in the dark override, so it is undefined in
    // every other theme and must fail.
    expect(failures[2]).toContain("bad.tsx:3");
    expect(failures[2]).toContain("--dark-only");
    // Same token twice on one line reports once.
    expect(failures[3]).toContain("bad.tsx:4");
    expect(failures[3]).toContain("--space-9");
    // A commented-out definition does not count as defined.
    expect(failures[4]).toContain("bad.tsx:5");
    expect(failures[4]).toContain("--ghost");
  });

  it("keeps later line numbers stable when a comment spans lines", () => {
    const root = makeRepo({
      "packages/kit/src/tokens.css": TOKENS_CSS,
      "packages/kit/src/kit.css": KIT_CSS,
      "apps/web_new/src/multiline.tsx": [
        "/*",
        " * a three-line comment",
        " * mentioning p-(--space-5)",
        " */",
        'const a = "gap-(--space-5)";',
        "",
      ].join("\n"),
    });

    const failures = runTokenCheck(options(root));

    expect(failures).toHaveLength(1);
    expect(failures[0]).toContain("multiline.tsx:5");
  });

  it("tolerates a missing tree and a missing kit.css", () => {
    const root = makeRepo({ "packages/kit/src/tokens.css": TOKENS_CSS });

    expect(runTokenCheck(options(root))).toEqual([]);
    expect(runTokenCheck({ ...options(root), trees: ["apps/web_new/src"] })).toEqual([]);
  });

  it("main returns 0 for a clean tree and 1 for an undefined token", () => {
    const clean = makeRepo({
      "packages/kit/src/tokens.css": TOKENS_CSS,
      "packages/kit/src/kit.css": KIT_CSS,
      "apps/web_new/src/ok.tsx": 'const a = "p-(--space-4)";\n',
    });
    const dirty = makeRepo({
      "packages/kit/src/tokens.css": TOKENS_CSS,
      "packages/kit/src/kit.css": KIT_CSS,
      "apps/web_new/src/bad.tsx": 'const a = "p-(--space-5)";\n',
    });

    expect(main(options(clean))).toBe(0);
    expect(main(options(dirty))).toBe(1);
  });

  it("runs as a CLI when invoked directly", () => {
    expect(isCliEntry(script)).toBe(true);
    expect(isCliEntry(join(dirname(script), "other.mjs"))).toBe(false);
    expect(isCliEntry(undefined)).toBe(false);
    expect(() => execFileSync(process.execPath, [script], { encoding: "utf8" })).not.toThrow();
  });
});
