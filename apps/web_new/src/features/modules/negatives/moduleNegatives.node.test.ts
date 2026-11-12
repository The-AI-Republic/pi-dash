// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Build negatives pinned at the source (parity/specs/modules.md §11.3,
// §11.4; MOD-042, MOD-043): no module file reads the target the app runs
// as, and no module file consults an edition gate, flag or address. The
// audit covers the feature, its four routes, the settings page for its
// switch, and the two shared surfaces that name modules (the palette's
// module context and the module picker). Fixtures below prove each token
// family flags real usage while passing the prose that names the absences.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { edition } from "../../../core/edition/index.js";

const here = dirname(fileURLToPath(import.meta.url));
const SRC = join(here, "..", "..", "..");
const PROJECT_ROUTES = join(SRC, "routes", "$ws", "_gated", "projects", "$projectId");

/** Comment-free body: prose may name the absences, code may not. */
function scanBody(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[\s;])\/\/[^\n]*/g, "$1");
}

function hits(source: string, tokens: RegExp[]): string[] {
  return tokens.filter((token) => token.test(source)).map((token) => token.source);
}

/**
 * Desktop-only platform members and build-specific paths (MOD-042). The
 * shared open/share pair is the only platform member a module screen may
 * use (copy link, open in new tab); it is pinned separately below.
 */
const DESKTOP_TOKENS = [
  /\b__TAURI__\b/,
  /platform-target/,
  /\/tauri\.js/,
  /\bisDesktop\b/,
  /\bisDesktopShell\b/,
  /\bdesktopOnly\b/,
  /\bdesktopVersion\b/,
  /\bagentRuntime\b/,
  /\blocalChat\b/,
  /\bshellSignIn\b/,
  /\bupdaterApi\b/i,
  /\bplatform\.(?!openExternal\b|shareableUrl\b)[A-Za-z]+/,
];

/**
 * Edition gates, flags and edition-owned addresses (MOD-043). A module
 * surface that asks an edition whether it exists, or calls a billing,
 * license or upgrade endpoint, would show a capability that differs by
 * build; the spec says none does.
 */
const EDITION_TOKENS = [
  /@pidash\/edition/,
  /core\/edition/,
  /\bedition\.(flags|routes|sidebar|settingsSections|api|slots|auth)\b/,
  /\bisEditionRouteEnabled\b/,
  /\bisEditionPublicRouteEnabled\b/,
  /\beditionRoute\b/,
  /\bEDITION_[A-Z_]+\b/,
  /\bcloudOnly\b/,
  /\bossOnly\b/,
  /\bcurrentPlan\b/,
  /\bsubscriptionTier\b/,
  /\blicenseKey\b/,
  /\/billing\//,
  /\/licen[cs]es?\//,
  /\/subscriptions?\//,
];

function sourcesUnder(directory: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(directory)) {
    const full = join(directory, name);
    if (statSync(full).isDirectory()) {
      out.push(...sourcesUnder(full));
    } else if (/\.(ts|tsx)$/.test(name) && !/\.test\.tsx?$/.test(name)) {
      out.push(full);
    }
  }
  return out;
}

/** The four addresses modules own, plus the switch's settings page. */
const MODULE_ROUTES = [
  join(PROJECT_ROUTES, "modules", "index.tsx"),
  join(PROJECT_ROUTES, "modules", "$moduleId.tsx"),
  join(PROJECT_ROUTES, "archives", "modules", "index.tsx"),
  join(SRC, "routes", "$ws", "_gated", "settings", "projects", "$projectId", "features", "modules.tsx"),
];

/** Every source the negatives cover: the feature, its routes, its shared homes. */
const entries = [
  ...sourcesUnder(join(SRC, "features", "modules")),
  ...MODULE_ROUTES,
  join(SRC, "shared", "shell", "contextual", "modules.ts"),
  join(SRC, "shared", "pickers", "modules.ts"),
];

const short = (path: string): string => relative(SRC, path);

describe("modules scan scope", () => {
  it("covers the feature, its routes and the shared module surfaces", () => {
    expect(entries.length).toBeGreaterThan(40);
    expect(entries.some((file) => file.endsWith(join("features", "modules", "api", "queries.ts")))).toBe(true);
    expect(
      entries.some((file) => file.endsWith(join("features", "modules", "components", "ModulesListScreen.tsx")))
    ).toBe(true);
    for (const route of MODULE_ROUTES) expect(entries, short(route)).toContain(route);
    expect(entries).toContain(join(SRC, "shared", "pickers", "modules.ts"));
    expect(entries).toContain(join(SRC, "shared", "shell", "contextual", "modules.ts"));
  });

  it("leaves tests and non-code files out of the audit", () => {
    for (const file of entries) {
      expect(short(file), short(file)).not.toMatch(/\.test\.tsx?$/);
      expect(short(file), short(file)).toMatch(/\.(ts|tsx)$/);
    }
  });

  it("declares exactly the module routes this area serves", () => {
    // A route added here has to be added to the list above, so a
    // build-specific module page cannot slip in unreviewed.
    const directories = [join(PROJECT_ROUTES, "modules"), join(PROJECT_ROUTES, "archives", "modules")];
    const routeFiles = directories.flatMap((directory) =>
      readdirSync(directory)
        .filter((name) => name.endsWith(".tsx") && !name.endsWith(".test.tsx"))
        .map((name) => join(directory, name))
    );
    expect(routeFiles.map(short).sort()).toEqual(
      [MODULE_ROUTES[0], MODULE_ROUTES[1], MODULE_ROUTES[2]].map((file) => short(file as string)).sort()
    );
  });
});

describe("modules desktop scan (MOD-042)", () => {
  it("flags desktop-only members and build paths", () => {
    expect(hits(scanBody("platform.agentRuntime?.isAvailable()"), DESKTOP_TOKENS)).not.toEqual([]);
    expect(hits(scanBody("const shell = window.__TAURI__"), DESKTOP_TOKENS)).not.toEqual([]);
    expect(hits(scanBody('import { platform } from "@pidash/platform-target"'), DESKTOP_TOKENS)).not.toEqual([]);
    expect(hits(scanBody('import { isDesktop } from "../core/platform/index.js"'), DESKTOP_TOKENS)).not.toEqual([]);
    expect(hits(scanBody("if (platform.isDesktop) hideBanner()"), DESKTOP_TOKENS)).not.toEqual([]);
  });

  it("passes the shared open/share pair and prose", () => {
    expect(hits(scanBody("platform.openExternal(platform.shareableUrl(href))"), DESKTOP_TOKENS)).toEqual([]);
    expect(hits(scanBody("// no desktop runtime on the modules surface"), DESKTOP_TOKENS)).toEqual([]);
  });

  it("finds no desktop-only usage in module sources", () => {
    for (const file of entries) {
      expect(hits(scanBody(readFileSync(file, "utf8")), DESKTOP_TOKENS), short(file)).toEqual([]);
    }
  });

  it("touches no platform member at all today", () => {
    const members = new Set<string>();
    for (const file of entries) {
      for (const match of scanBody(readFileSync(file, "utf8")).matchAll(/\bplatform\.([A-Za-z]+)/g)) {
        members.add(match[1] ?? "");
      }
    }
    // Copy link and open in new tab build addresses from the module entry
    // helpers, not from a platform shim, so the set is empty.
    expect([...members]).toEqual([]);
  });
});

describe("modules edition scan (MOD-043)", () => {
  it("flags edition gates, flags and edition-owned addresses", () => {
    expect(hits(scanBody('import { edition } from "@pidash/edition"'), EDITION_TOKENS)).not.toEqual([]);
    expect(hits(scanBody("if (edition.flags['module-analytics']) show()"), EDITION_TOKENS)).not.toEqual([]);
    expect(hits(scanBody("isEditionRouteEnabled(edition, 'modules')"), EDITION_TOKENS)).not.toEqual([]);
    expect(hits(scanBody('client.get("/api/billing/subscriptions/")'), EDITION_TOKENS)).not.toEqual([]);
    expect(hits(scanBody("const cloudOnly = true;"), EDITION_TOKENS)).not.toEqual([]);
  });

  it("passes prose naming the editions and ordinary module vocabulary", () => {
    expect(hits(scanBody("// favorites, filters and links render in every edition"), EDITION_TOKENS)).toEqual([]);
    expect(hits(scanBody('const status = "planned";'), EDITION_TOKENS)).toEqual([]);
    expect(hits(scanBody("const plan = { start_date, target_date };"), EDITION_TOKENS)).toEqual([]);
  });

  it("finds no edition gate in module sources", () => {
    for (const file of entries) {
      expect(hits(scanBody(readFileSync(file, "utf8")), EDITION_TOKENS), short(file)).toEqual([]);
    }
  });

  it("serves the module addresses from the base app, not from an edition", () => {
    // Modules are declared by no edition: nothing can make a module page
    // appeared or disappear by build. An edition that listed a module
    // address here would be a gate the spec forbids.
    const paths = (edition.routes ?? []).filter((route) => /module/i.test(route.path));
    expect(paths).toEqual([]);
    const sidebar = (edition.sidebar ?? []).filter((entry) => /module/i.test(entry.to));
    expect(sidebar).toEqual([]);
    const sections = (edition.settingsSections ?? []).filter((section) => /module/i.test(section.id));
    expect(sections).toEqual([]);
  });
});

describe("module route independence", () => {
  it("declares every module route as a plain workspace address", () => {
    for (const route of MODULE_ROUTES) {
      const body = scanBody(readFileSync(route, "utf8"));
      expect(body, short(route)).toContain("createFileRoute(");
      expect(body, short(route)).toMatch(/createFileRoute\("\/\$ws\//);
      // No conditional render, loader or guard keyed on a build.
      expect(body, short(route)).not.toMatch(/\bif\s*\(\s*(isDesktop|edition|__TAURI__)/);
    }
  });
});
