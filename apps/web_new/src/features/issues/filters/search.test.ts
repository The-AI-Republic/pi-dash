// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import { DEFAULT_ISSUE_SEARCH, parseIssueSearch } from "./search.js";

describe("issue search", () => {
  it("defaults an empty search to the list layout", () => {
    expect(parseIssueSearch({})).toEqual({ layout: "list" });
    expect(parseIssueSearch(undefined)).toEqual(DEFAULT_ISSUE_SEARCH);
  });

  it("keeps the compact layout", () => {
    expect(parseIssueSearch({ layout: "compact" })).toEqual({ layout: "compact" });
  });

  it("falls back to defaults on unknown layouts and shapes", () => {
    expect(parseIssueSearch({ layout: "board" })).toEqual(DEFAULT_ISSUE_SEARCH);
    expect(parseIssueSearch({ layout: 42 })).toEqual(DEFAULT_ISSUE_SEARCH);
    expect(parseIssueSearch("list")).toEqual(DEFAULT_ISSUE_SEARCH);
    expect(parseIssueSearch(null)).toEqual(DEFAULT_ISSUE_SEARCH);
  });
});
