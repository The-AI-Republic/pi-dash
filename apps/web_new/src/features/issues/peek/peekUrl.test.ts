// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

import { describe, expect, it } from "vitest";

import { nestingSearchFor, parseNestingLevel, stripPeekParams } from "./peekUrl.js";

describe("stripPeekParams", () => {
  it("removes every peek param and keeps the rest", () => {
    expect(stripPeekParams({ layout: "list", peekIssueId: "a", peekProjectId: "b", peekNestingLevel: "1" })).toEqual({
      layout: "list",
    });
  });

  it("leaves a peekless search alone", () => {
    expect(stripPeekParams({ layout: "list" })).toEqual({ layout: "list" });
  });
});

describe("nestingSearchFor", () => {
  it("writes the level and keeps the rest", () => {
    expect(nestingSearchFor({ peekIssueId: "a" }, 2)).toEqual({ peekIssueId: "a", peekNestingLevel: "2" });
  });

  it("strips the level at zero and below", () => {
    expect(nestingSearchFor({ peekIssueId: "a", peekNestingLevel: "2" }, 0)).toEqual({ peekIssueId: "a" });
    expect(nestingSearchFor({ peekIssueId: "a", peekNestingLevel: "2" }, -1)).toEqual({ peekIssueId: "a" });
  });
});

describe("parseNestingLevel", () => {
  it.each([
    [undefined, 0],
    [null, 0],
    ["", 0],
    ["1", 1],
    ["3", 3],
    [2, 2],
    ["nope", 0],
    ["-1", 0],
    ["1.5", 1],
    [{}, 0],
  ])("parses %s as %s", (raw, expected) => {
    expect(parseNestingLevel(raw)).toBe(expected);
  });
});
