// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edition module resolution: the default, the override, the fail-fast,
// and the outside-the-repo serve allowance.
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { defaultEditionModule, resolveEditionModule } from "./edition-module.mjs";

const scratchDirs = [];

afterEach(() => {
  while (scratchDirs.length > 0) rmSync(scratchDirs.pop(), { recursive: true, force: true });
});

function scratchApp() {
  const root = mkdtempSync(join(tmpdir(), "edition-module-"));
  scratchDirs.push(root);
  const appDir = join(root, "repo", "apps", "web_new");
  mkdirSync(join(appDir, "src", "core", "edition"), { recursive: true });
  writeFileSync(join(appDir, "src", "core", "edition", "oss.ts"), "export const edition = {};\n");
  return appDir;
}

describe("defaultEditionModule", () => {
  it("points at the OSS module in the app dir", () => {
    const appDir = scratchApp();
    expect(defaultEditionModule(appDir)).toBe(resolve(appDir, "src/core/edition/oss.ts"));
  });
});

describe("resolveEditionModule", () => {
  it("resolves the OSS module with no fs allowance when unset", () => {
    const appDir = scratchApp();
    expect(resolveEditionModule({}, appDir)).toEqual({
      modulePath: resolve(appDir, "src/core/edition/oss.ts"),
      fsAllow: null,
    });
  });

  it("treats a blank override as unset", () => {
    const appDir = scratchApp();
    expect(resolveEditionModule({ PIDASH_EDITION_MODULE: "  " }, appDir).fsAllow).toBeNull();
  });

  it("resolves a relative override against the app dir", () => {
    const appDir = scratchApp();
    const custom = join(appDir, "src", "core", "edition", "custom.ts");
    writeFileSync(custom, "export const edition = {};\n");
    expect(resolveEditionModule({ PIDASH_EDITION_MODULE: "src/core/edition/custom.ts" }, appDir)).toEqual({
      modulePath: custom,
      fsAllow: null,
    });
  });

  it("resolves an absolute override inside the repo with no fs allowance", () => {
    const appDir = scratchApp();
    const custom = join(appDir, "src", "core", "edition", "custom.ts");
    writeFileSync(custom, "export const edition = {};\n");
    expect(resolveEditionModule({ PIDASH_EDITION_MODULE: custom }, appDir)).toEqual({
      modulePath: custom,
      fsAllow: null,
    });
  });

  it("allows serving an override outside the repo", () => {
    const appDir = scratchApp();
    const outside = mkdtempSync(join(tmpdir(), "edition-outside-"));
    scratchDirs.push(outside);
    const custom = join(outside, "cloud.ts");
    writeFileSync(custom, "export const edition = {};\n");
    expect(resolveEditionModule({ PIDASH_EDITION_MODULE: custom }, appDir)).toEqual({
      modulePath: custom,
      fsAllow: [resolve(appDir, "..", ".."), outside],
    });
  });

  it("fails fast on a missing override file", () => {
    const appDir = scratchApp();
    const missing = join(appDir, "src", "core", "edition", "nope.ts");
    expect(() => resolveEditionModule({ PIDASH_EDITION_MODULE: missing }, appDir)).toThrow(
      `[edition] PIDASH_EDITION_MODULE points at a missing file: ${missing}`
    );
  });
});
