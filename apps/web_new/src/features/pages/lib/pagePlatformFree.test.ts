// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop absence in the pages area (parity/specs/pages.md §9.4, PAGE-081):
// desktop runs the same experience as web, so no module under
// features/pages may read the desktop platform bridge for behaviour. The
// check walks the area's own sources and refuses the bridge (its type-only
// KeyValueStore import aside) and any desktop-only marker. Edition slots
// are not desktop branching: the cloud edition legitimately fills the
// move entry and pane slot (PAGE-017/078). It is a deliberate source
// assertion rather than a runtime one: behaviour that cannot exist cannot
// be tested by driving it.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const PAGES_ROOT = fileURLToPath(new URL("..", import.meta.url));

function sourceFiles(dir: string): string[] {
  const found: string[] = [];
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) {
      found.push(...sourceFiles(path));
    } else if (/\.(ts|tsx)$/.test(entry) && !/\.test\.(ts|tsx)$/.test(entry)) {
      found.push(path);
    }
  }
  return found;
}

const FILES = sourceFiles(PAGES_ROOT).map((path) => ({ path, source: readFileSync(path, "utf8") }));

const FORBIDDEN: Array<{ name: string; pattern: RegExp }> = [
  // The bridge's value module is the behaviour path; `types.js` is only
  // the KeyValueStore shape the preference reader already takes.
  { name: "the desktop platform bridge", pattern: /core\/platform\/(?!types\.js)/ },
  { name: "a desktop-only marker", pattern: /data-desktop-only|isDesktopApp|desktopOnly/ },
  { name: "a mobile-only branch", pattern: /isMobileApp|data-mobile-only/ },
];

describe("pages area has no edition or desktop branch (PAGE-081)", () => {
  it("scans a non-trivial set of page sources", () => {
    expect(FILES.length).toBeGreaterThan(20);
  });

  it.each(FORBIDDEN)("imports no $name", ({ pattern }) => {
    const offenders = FILES.filter((file) => pattern.test(file.source)).map((file) => file.path);
    expect(offenders).toEqual([]);
  });
});
