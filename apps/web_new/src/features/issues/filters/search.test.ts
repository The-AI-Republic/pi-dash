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
});
