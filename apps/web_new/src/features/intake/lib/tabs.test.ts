// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { DEFAULT_INTAKE_TAB, INTAKE_TABS, parseIntakeTab } from "./tabs.js";

describe("intake tab defaults (INT-012)", () => {
  it("lists Open then Closed", () => {
    expect(INTAKE_TABS).toEqual(["open", "closed"]);
    expect(DEFAULT_INTAKE_TAB).toBe("open");
  });
});

describe("parseIntakeTab (INT-028)", () => {
  it("keeps the two valid values", () => {
    expect(parseIntakeTab("open")).toBe("open");
    expect(parseIntakeTab("closed")).toBe("closed");
  });

  it("treats anything else as the default tab", () => {
    for (const raw of [undefined, null, "", "OPEN", "archived", 1, {}, ["closed"]]) {
      expect(parseIntakeTab(raw)).toBe("open");
    }
  });
});
