// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Scoped dialog title tests (parity/specs/analytics.md §13, ANL-014).
import { describe, expect, it } from "vitest";

import { scopedDialogTitle } from "./dialogTitle.js";

describe("scopedDialogTitle", () => {
  it("names the project when nothing narrows it", () => {
    expect(scopedDialogTitle({ projectName: "Website" })).toBe("Analytics for Website");
  });

  it("appends the cycle once", () => {
    expect(scopedDialogTitle({ projectName: "Website", cycleName: "Sprint 3" })).toBe(
      "Analytics for Website in Sprint 3"
    );
  });

  it("appends the module with a single space, as intended", () => {
    // The old template left an empty cycle fragment behind, so the oracle
    // pins a double space; the new build corrects it (spec §13, §18).
    expect(scopedDialogTitle({ projectName: "Website", moduleName: "Alpha" })).toBe("Analytics for Website in Alpha");
  });

  it("treats a null narrowing as no narrowing", () => {
    expect(scopedDialogTitle({ projectName: "Website", cycleName: null, moduleName: null })).toBe(
      "Analytics for Website"
    );
    expect(scopedDialogTitle({ projectName: "Website", moduleName: null })).toBe("Analytics for Website");
  });

  it("prefers the cycle when both are given, matching the server", () => {
    expect(scopedDialogTitle({ projectName: "Website", cycleName: "Sprint 3", moduleName: "Alpha" })).toBe(
      "Analytics for Website in Sprint 3"
    );
  });
});
