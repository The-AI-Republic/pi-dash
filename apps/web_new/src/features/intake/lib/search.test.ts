// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  DEFAULT_INTAKE_SEARCH,
  intakeSearchFor,
  parseIntakeSearch,
  settleIntakeSearch,
  withoutSelection,
} from "./search.js";

describe("parseIntakeSearch (INT-001/INT-028)", () => {
  it("keeps both valid keys", () => {
    expect(parseIntakeSearch({ currentTab: "closed", inboxIssueId: "i-1" })).toEqual({
      currentTab: "closed",
      inboxIssueId: "i-1",
    });
  });

  it("defaults to Open and no selection", () => {
    expect(parseIntakeSearch({})).toEqual({ currentTab: "open" });
    expect(DEFAULT_INTAKE_SEARCH).toEqual({ currentTab: "open" });
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

describe("writing the address (INT-004/INT-005/INT-028)", () => {
  it("always writes the tab and only adds an id when one exists", () => {
    expect(intakeSearchFor("closed", "i-1")).toEqual({ currentTab: "closed", inboxIssueId: "i-1" });
    expect(intakeSearchFor("closed", null)).toEqual({ currentTab: "closed" });
    expect(intakeSearchFor("open", undefined)).toEqual({ currentTab: "open" });
    expect(intakeSearchFor("open", "")).toEqual({ currentTab: "open" });
  });

  it("drops the selection and keeps the tab", () => {
    expect(withoutSelection({ currentTab: "closed", inboxIssueId: "i-1" })).toEqual({ currentTab: "closed" });
  });
});

describe("settleIntakeSearch (INT-005/INT-028)", () => {
  it("keeps an id the settled list contains (INT-028 step 2)", () => {
    expect(settleIntakeSearch({ search: { currentTab: "open", inboxIssueId: "i-2" }, rowIds: ["i-1", "i-2"] })).toBe(
      null
    );
  });

  it("auto-selects the first row when the address names none (INT-005)", () => {
    expect(settleIntakeSearch({ search: { currentTab: "open" }, rowIds: ["i-1", "i-2"] })).toEqual({
      currentTab: "open",
      inboxIssueId: "i-1",
    });
  });

  it("replaces a stale or cross-tab id with the first row (INT-028 step 3)", () => {
    expect(settleIntakeSearch({ search: { currentTab: "closed", inboxIssueId: "gone" }, rowIds: ["c-1"] })).toEqual({
      currentTab: "closed",
      inboxIssueId: "c-1",
    });
  });

  it("clears a stale id when the list is empty", () => {
    expect(settleIntakeSearch({ search: { currentTab: "closed", inboxIssueId: "gone" }, rowIds: [] })).toEqual({
      currentTab: "closed",
    });
  });

  it("selects nothing for an empty tab (INT-005)", () => {
    expect(settleIntakeSearch({ search: { currentTab: "open" }, rowIds: [] })).toBe(null);
  });
});
