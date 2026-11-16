// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { parseIntakeSearch } from "./search.js";

describe("parseIntakeSearch (INT-001/INT-028)", () => {
  it("keeps both valid keys", () => {
    expect(parseIntakeSearch({ currentTab: "closed", inboxIssueId: "i-1" })).toEqual({
      currentTab: "closed",
      inboxIssueId: "i-1",
    });
  });

  it("defaults to Open and no selection", () => {
    expect(parseIntakeSearch({})).toEqual({ currentTab: "open" });
  });

  it("drops unknown keys and malformed tabs", () => {
    expect(parseIntakeSearch({ currentTab: "weird", other: "x" })).toEqual({ currentTab: "open" });
    expect(parseIntakeSearch(null)).toEqual({ currentTab: "open" });
    expect(parseIntakeSearch("closed")).toEqual({ currentTab: "open" });
  });

  it("trims ids and drops blanks and non-strings", () => {
    expect(parseIntakeSearch({ inboxIssueId: "  i-2  " })).toEqual({ currentTab: "open", inboxIssueId: "i-2" });
    expect(parseIntakeSearch({ inboxIssueId: "   " })).toEqual({ currentTab: "open" });
    expect(parseIntakeSearch({ inboxIssueId: 7 })).toEqual({ currentTab: "open" });
  });
});
