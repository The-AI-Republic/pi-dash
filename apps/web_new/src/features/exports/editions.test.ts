// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edition/desktop sweep for the export area (EXP-024, NEWFRONT-773). The app
// has one source tree for web and desktop and one @pidash/edition module per
// build. The export surfaces must therefore render ungated everywhere: no
// export source may read the edition, the build target, licence, billing,
// upgrade or desktop state, and no overlay/desktop fork of export code may
// exist. This is the source half of the row; the oracle proves the render and
// traffic half in a browser.
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const HERE = dirname(fileURLToPath(import.meta.url));
const SRC = join(HERE, "..", "..");
const REPO = join(SRC, "..", "..", "..");

/** Comment-free body: prose may name editions, code may not read them. */
function scanBody(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[\s;])\/\/[^\n]*/g, "$1");
}

function sourceFiles(directory: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(directory, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile() || !/\.(ts|tsx)$/.test(entry.name)) continue;
    if (/\.(test|spec)\.tsx?$/.test(entry.name)) continue;
    if (entry.name === "routeTree.gen.ts") continue;
    out.push(join(entry.parentPath, entry.name));
  }
  return out;
}

/** Tokens that would indicate a per-edition or per-build fork. */
const GATE_TOKENS: Array<[RegExp, string]> = [
  // `platform.fetch` is the shared transport (the PDF renderer uses it for
  // image bytes in both builds); only a target KIND branch is a fork.
  [/from\s+["'][^"']*core\/edition/, "edition import"],
  [/@pidash\/edition/, "edition module"],
  [/@pidash\/platform-target/, "platform target module"],
  [/\bedition\.\w/, "edition read"],
  [/\bplatform\.kind\b/, "platform target branch"],
  [/\b(?:isEdition\w*|isDesktop\w*|editionGate\w*|desktopOnly\w*|editionOnly\w*)\b/, "gate branch"],
  [/\b(?:UpgradePill|PaywallGate|EditionGate|UpgradeGate)\b/, "gate component"],
  [/\b(?:license|licence)_?(?:required|gate)\b/i, "licence gate"],
  [/\b(?:billing|premium|upgrade)_?(?:required|gate|only)\b/i, "paid gate"],
];

/** Directories that implement one export surface each. */
const SURFACES: Record<string, string[]> = {
  "workspace export settings": [
    join(SRC, "features", "exports"),
    join(SRC, "features", "workspace-settings", "components", "ExportsSettingsPage.tsx"),
    join(SRC, "features", "workspace-settings", "lib", "exportsGuide.ts"),
  ],
  "previous exports history": [join(SRC, "features", "exports")],
  "analytics export actions": [
    join(SRC, "features", "analytics", "components", "AnalyticsTable.tsx"),
    join(SRC, "features", "analytics", "components", "StateBreakdownSection.tsx"),
    join(SRC, "features", "analytics", "components", "CustomInsightsSection.tsx"),
    join(SRC, "shared", "export"),
  ],
  "page-export dialog": [join(SRC, "features", "pages-export")],
  "profile activity download": [
    join(SRC, "features", "profile", "components", "ProfileActivityDownload.tsx"),
    join(SRC, "features", "profile", "api", "export.ts"),
  ],
};

function surfaceFiles(paths: string[]): string[] {
  return paths.flatMap((path) => {
    try {
      return readdirSync(path).length >= 0 ? sourceFiles(path) : [];
    } catch {
      // A single-file surface is walked through its parent directory.
      return /\.(ts|tsx)$/.test(path) && !/\.(test|spec)\.tsx?$/.test(path) ? [path] : [];
    }
  });
}

describe("export surfaces are ungated across editions and desktop (EXP-024)", () => {
  for (const [surface, paths] of Object.entries(SURFACES)) {
    it(`${surface} reads no edition, licence, billing, upgrade or desktop branch`, () => {
      const files = surfaceFiles(paths);
      expect(files.length).toBeGreaterThan(0);
      const offenders = files.flatMap((file) => {
        const body = scanBody(readFileSync(file, "utf8"));
        return GATE_TOKENS.filter(([token]) => token.test(body)).map(
          ([, label]) => `${relative(REPO, file)}: ${label}`
        );
      });
      expect(offenders).toEqual([]);
    });
  }

  it("has no overlay or desktop fork of the export code", () => {
    const overlayRoots = [join(REPO, "desktop-overlay"), join(REPO, "ee-overlay")];
    const markers = /export-issues|Exporting project|Previous exports|Page exported successfully|profile-activity-/;
    const offenders: string[] = [];
    for (const root of overlayRoots) {
      let files: string[] = [];
      try {
        files = sourceFiles(root);
      } catch {
        continue; // Absent in some checkouts; the main tree is still swept.
      }
      for (const file of files) {
        if (markers.test(scanBody(readFileSync(file, "utf8")))) offenders.push(relative(REPO, file));
      }
    }
    expect(offenders).toEqual([]);
  });

  it("keeps the export surfaces free of gate markers in rendered text", () => {
    const markers = /\b(?:Upgrade to|paywall|desktop-only|cloud-only|licen[cs]e required)\b/i;
    const files = surfaceFiles(Object.values(SURFACES).flat());
    const offenders = files.filter((file) => markers.test(scanBody(readFileSync(file, "utf8"))));
    expect(offenders.map((file) => relative(REPO, file))).toEqual([]);
  });
});
