// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Static public-subtree boundary (SPC-001, SPC-018): loading a space route
// must never reach the workspace shell, session gate, old frontend, or a
// `@pi-dash/*` runtime import. This walks the actual import graph rather
// than trusting one barrel to stay narrow.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(here, "../../..");

function filesUnder(directory: string): string[] {
  return readdirSync(directory).flatMap((entry) => {
    const path = join(directory, entry);
    return statSync(path).isDirectory() ? filesUnder(path) : [path];
  });
}

const entries = [
  ...filesUnder(join(ROOT, "src/features/spaces")),
  ...filesUnder(join(ROOT, "src/routes/spaces")),
].filter((path) => /\.(ts|tsx)$/.test(path) && !/\.test\.tsx?$/.test(path));

const imports = (path: string): string[] => {
  const body = readFileSync(path, "utf8");
  return Array.from(body.matchAll(/(?:from\s+|import\s*\()["']([^"']+)["']/g), (match) => match[1] ?? "");
};

const forbidden = (specifier: string): boolean =>
  specifier.startsWith("@pi-dash/") ||
  specifier.startsWith("apps/") ||
  specifier.startsWith("desktop-overlay/") ||
  specifier.includes("shared/shell/index") ||
  specifier.includes("core/session") ||
  specifier.includes("features/instance/queries") ||
  specifier.includes("AppShell") ||
  specifier.includes("features/issues");

describe("space subtree boundary", () => {
  it("has entries to inspect", () => {
    expect(entries.length).toBeGreaterThan(10);
  });

  it("never imports the workspace shell, session, old app or @pi-dash runtime packages", () => {
    const offenders = entries.flatMap((path) =>
      imports(path)
        .filter((specifier) => forbidden(specifier))
        .map((specifier) => `${relative(ROOT, path)} -> ${specifier}`)
    );
    expect(offenders).toEqual([]);
  });

  it("keeps the shared not-found and document helpers on leaf modules", () => {
    const offenders = entries.flatMap((path) =>
      imports(path)
        .filter((specifier) => specifier.includes("shared/shell/") && !/\/[A-Za-z]+\.js$/.test(specifier))
        .map((specifier) => `${relative(ROOT, path)} -> ${specifier}`)
    );
    expect(offenders).toEqual([]);
  });
});
