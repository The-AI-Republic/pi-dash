// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import { parseIssueSearch } from "./search.js";

describe("issue search", () => {
  it("leaves an empty search absent so the param stays out of the URL", () => {
    expect(parseIssueSearch({})).toEqual({});
    expect(parseIssueSearch(undefined)).toEqual({});
  });

  it("keeps explicit layouts", () => {
    expect(parseIssueSearch({ layout: "compact" })).toEqual({ layout: "compact" });
    expect(parseIssueSearch({ layout: "list" })).toEqual({ layout: "list" });
  });

  it("falls back to absent on unknown layouts and shapes", () => {
    expect(parseIssueSearch({ layout: "board" })).toEqual({});
    expect(parseIssueSearch({ layout: 42 })).toEqual({});
    expect(parseIssueSearch("list")).toEqual({});
    expect(parseIssueSearch(null)).toEqual({});
  });

  it("keeps the peek selection, project, and nesting level (ISS-167)", () => {
    expect(parseIssueSearch({ peekIssueId: "WEB-42" })).toEqual({ peekIssueId: "WEB-42" });
    expect(parseIssueSearch({ peekIssueId: "a", peekProjectId: "b", peekNestingLevel: "1" })).toEqual({
      peekIssueId: "a",
      peekProjectId: "b",
      peekNestingLevel: "1",
    });
    expect(parseIssueSearch({ peekNestingLevel: 2 })).toEqual({});
  });

  it("drops display and filter keys: display state is never URL deep-linkable (ISS-106)", () => {
    expect(
      parseIssueSearch({
        layout: "list",
        group_by: "state",
        sub_group_by: "priority",
        order_by: "-created_at",
        show_empty_groups: true,
        sub_issue: true,
        display_properties: { state: false },
      })
    ).toEqual({ layout: "list" });
  });
});
