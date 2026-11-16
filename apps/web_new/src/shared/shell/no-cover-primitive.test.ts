// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
//
// SHELL-105 (NEWFRONT-294): negative TARGET row — no cover-image primitive
// exists anywhere in the new app (target per main#523). Cards and detail
// headers render no cover slot, shimmer, remote-URL cover, or fallback art.
// This guard fails if any non-test rendering source introduces a cover
// component or reads the server's cover fields. Out of scope: the API client
// (its schemas model the server payloads, covers included), test fixtures,
// and the parity reads in e2e/. Shared fixture modules (testing.*,
// *fixture*, testing/ and __fixtures__/ trees) count as test code: they
// build full server payloads, so they must spell every required field,
// covers included.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const HERE = new URL(".", import.meta.url);
const TREES = [new URL("../../", HERE), new URL("../../../../../packages/kit/src/", HERE)];

function sourceFiles(root: URL): string[] {
  const out: string[] = [];
  const entries = readdirSync(root, { recursive: true, withFileTypes: true });
  for (const entry of entries) {
    if (!entry.isFile()) continue;
    if (!/\.(ts|tsx)$/.test(entry.name)) continue;
    if (/\.(test|stories)\.tsx?$/.test(entry.name)) continue;
    // Shared test-fixture modules are test code, not rendering sources.
    if (/^testing\.tsx?$/.test(entry.name)) continue;
    if (/fixture/i.test(entry.name)) continue;
    const full = join(entry.parentPath, entry.name);
    if (/\/(testing|__fixtures__)\//.test(full)) continue;
    out.push(full);
  }
  return out;
}

describe("no cover-image primitive (SHELL-105)", () => {
  it("has no cover-named source file", () => {
    const offenders: string[] = [];
    for (const root of TREES) {
      for (const file of sourceFiles(root)) {
        const base = file.split("/").pop() ?? "";
        // "recovery" (auth) is unrelated vocabulary, not a cover primitive.
        if (/(?<!re)cover/i.test(base)) offenders.push(file);
      }
    }
    expect(offenders).toEqual([]);
  });

  it("reads no cover field and defines no cover component", () => {
    const pattern = /\bcover_image(_asset|_url)?\b|\b[Cc]overImage\b|\bProjectCover\b/;
    const offenders: string[] = [];
    for (const root of TREES) {
      for (const file of sourceFiles(root)) {
        if (pattern.test(readFileSync(file, "utf8"))) offenders.push(file);
      }
    }
    expect(offenders).toEqual([]);
  });
});
