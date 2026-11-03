// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  nestingSearchFor,
  parseNestingLevel,
  peekNamesChild,
  peekNamesRoot,
  peekSearchFor,
  peekSearchFromLocation,
  peekSelectionFromSearch,
  replacePeekSearch,
  stripPeekParams,
} from "./peekUrl.js";

afterEach(() => {
  vi.unstubAllGlobals();
  globalThis.history.replaceState({}, "", "/");
});

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

describe("peekSelectionFromSearch", () => {
  it("reads the issue under the route project without an override", () => {
    expect(peekSelectionFromSearch({ peekIssueId: "issue-1", layout: "list" }, "route-project")).toEqual({
      issueId: "issue-1",
      projectId: "route-project",
      nestingLevel: 0,
    });
  });

  it("reads a foreign project and the nesting level", () => {
    expect(
      peekSelectionFromSearch(
        { peekIssueId: "issue-1", peekProjectId: "other-project", peekNestingLevel: "2" },
        "route-project"
      )
    ).toEqual({ issueId: "issue-1", projectId: "other-project", nestingLevel: 2 });
  });

  it("treats a blank project as the route project", () => {
    expect(peekSelectionFromSearch({ peekIssueId: "issue-1", peekProjectId: "" }, "route-project")).toEqual({
      issueId: "issue-1",
      projectId: "route-project",
      nestingLevel: 0,
    });
  });

  it.each([[{}], [{ peekIssueId: "" }], [{ peekIssueId: "   " }], [{ peekIssueId: 7 }]])(
    "reads no selection from %j",
    (search) => {
      expect(peekSelectionFromSearch(search as Record<string, unknown>, "route-project")).toBeNull();
    }
  );
});

describe("peekSearchFor", () => {
  it("writes a same-project selection without the project param", () => {
    expect(
      peekSearchFor(
        { layout: "list", peekIssueId: "old", peekProjectId: "stale", peekNestingLevel: "1" },
        { issueId: "issue-1", projectId: "route-project", nestingLevel: 0 },
        "route-project"
      )
    ).toEqual({ layout: "list", peekIssueId: "issue-1" });
  });

  it("writes a foreign project and the nesting level", () => {
    expect(
      peekSearchFor(
        { layout: "list" },
        { issueId: "issue-2", projectId: "other-project", nestingLevel: 2 },
        "route-project"
      )
    ).toEqual({ layout: "list", peekIssueId: "issue-2", peekProjectId: "other-project", peekNestingLevel: "2" });
  });
});

describe("peekSearchFromLocation", () => {
  it("reads the live query as a plain record", () => {
    globalThis.history.replaceState({}, "", "/acme/projects/p1/issues?layout=compact&peekIssueId=issue-1");
    expect(peekSearchFromLocation()).toEqual({ layout: "compact", peekIssueId: "issue-1" });
  });

  it("reads nothing when there is no document", () => {
    vi.stubGlobal("window", undefined);
    expect(peekSearchFromLocation()).toEqual({});
  });
});

describe("replacePeekSearch", () => {
  it("replaces the query and keeps the path and unrelated params", () => {
    globalThis.history.replaceState({}, "", "/acme/projects/p1/issues?layout=compact");
    replacePeekSearch({ layout: "compact", peekIssueId: "issue-1" });
    expect(globalThis.location.pathname).toBe("/acme/projects/p1/issues");
    expect(globalThis.location.search).toBe("?layout=compact&peekIssueId=issue-1");
  });

  it("leaves the address alone when there is no document", () => {
    globalThis.history.replaceState({}, "", "/acme/projects/p1/issues?layout=compact");
    vi.stubGlobal("window", undefined);
    expect(() => replacePeekSearch({ peekIssueId: "issue-1" })).not.toThrow();
  });

  it("drops params the address cannot spell", () => {
    globalThis.history.replaceState({}, "", "/acme/projects/p1/issues");
    replacePeekSearch({ layout: "compact", peekIssueId: "issue-1", page: 2, tags: null, nested: ["a"] });
    expect(globalThis.location.search).toBe("?layout=compact&peekIssueId=issue-1");
  });
});

describe("peekNamesRoot", () => {
  const root = { peeked: "issue-1", peekProjectId: null };

  it("matches the issue and the project together", () => {
    expect(peekNamesRoot(root, "issue-1", null)).toBe(true);
    expect(peekNamesRoot(root, "issue-2", null)).toBe(false);
    expect(peekNamesRoot(root, "issue-1", "other-project")).toBe(false);
  });
});

describe("peekNamesChild", () => {
  const child = { issueId: "issue-2", projectId: "other-project", nestingLevel: 1 };

  it("matches a child by its written-out foreign project", () => {
    expect(peekNamesChild(child, "route-project", "issue-2", "other-project")).toBe(true);
  });

  it("treats the route project as an absent parameter", () => {
    const sameProject = { issueId: "issue-2", projectId: "route-project", nestingLevel: 1 };
    expect(peekNamesChild(sameProject, "route-project", "issue-2", null)).toBe(true);
    expect(peekNamesChild(sameProject, "route-project", "issue-2", "route-project")).toBe(false);
  });

  it("never matches without a child or with another issue", () => {
    expect(peekNamesChild(null, "route-project", "issue-2", "other-project")).toBe(false);
    expect(peekNamesChild(child, "route-project", "issue-3", "other-project")).toBe(false);
  });
});
