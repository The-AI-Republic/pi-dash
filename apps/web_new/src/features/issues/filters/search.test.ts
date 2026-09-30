// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
