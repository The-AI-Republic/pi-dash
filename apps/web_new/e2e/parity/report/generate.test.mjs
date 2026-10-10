// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the parity report generator (run: pnpm --filter web_new
// test:parity-report). Inventory files may re-list capability rows in a
// trailing checklist section (e.g. notifications.md re-lists NTF-026-031);
// the report must count each row ID once, keeping the capability table's
// status (first occurrence wins), so a fully-green area reads N/N.
import { strict as assert } from "node:assert";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const generator = join(here, "generate.mjs");

function fixtureInventory() {
  const dir = mkdtempSync(join(tmpdir(), "parity-report-"));
  writeFileSync(
    join(dir, "widgets.md"),
    [
      "# Widgets",
      "",
      "| ID | Capability | Parity test | Status |",
      "| --- | --- | --- | --- |",
      "| WDG-001 | First widget | widgets/a.spec.ts | oracle green |",
      "| WDG-002 | Second widget | | todo |",
      "| WDG-003 | Third widget | widgets/b.spec.ts | oracle green |",
      "",
      "## Cross-cutting checklist",
      "",
      "| ID | Finding |",
      "| --- | --- |",
      "| WDG-001 | No role-gated branch in the widget code |",
      "| WDG-002 | oracle green |",
      "| WDG-003 | No realtime updates in the widget code |",
      "",
    ].join("\n")
  );
  return dir;
}

function generate(inventoryDir) {
  const out = join(mkdtempSync(join(tmpdir(), "parity-report-out-")), "report.md");
  execFileSync(process.execPath, [generator, "--inventory", inventoryDir, "--out", out], {
    stdio: "pipe",
  });
  return readFileSync(out, "utf8");
}

describe("parity report generator", () => {
  it("dedupes checklist re-listings so a fully-green area reads N/N", () => {
    const report = generate(fixtureInventory());
    assert.match(report, /^## widgets — 2\/3 oracle green, 0\/3 new green$/m);
  });

  it("keeps the first occurrence's status in both directions", () => {
    const report = generate(fixtureInventory());
    // WDG-001 is green in the capability table but its checklist re-listing
    // carries finding prose: still green.
    assert.match(report, /^\| WDG-001 \| green \| — \|/m);
    // WDG-002 is todo in the capability table but its checklist re-listing
    // happens to read "oracle green": still not green (first wins).
    assert.ok(!/^\| WDG-002 \| green \|/m.test(report));
  });

  it("lists each lit row ID exactly once in the per-row table", () => {
    const report = generate(fixtureInventory());
    for (const id of ["WDG-001", "WDG-003"]) {
      assert.equal(report.split(`| ${id} |`).length - 1, 1, `expected one table row for ${id}`);
    }
    // WDG-002 is todo with no runs, so the per-row table omits it entirely.
    assert.ok(!report.includes("| WDG-002 |"));
  });
});
