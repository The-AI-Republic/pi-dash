// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { describe, expect, it } from "vitest";

import { isMigratedPath, MIGRATED_ROUTE_PREFIXES, normalizePathPrefix, resolveLinkKind } from "./migrated-routes.js";

describe("normalizePathPrefix", () => {
  it("adds a leading slash and strips trailing slashes, queries and hashes", () => {
    expect(normalizePathPrefix("sign-in")).toBe("/sign-in");
    expect(normalizePathPrefix("/sign-in/")).toBe("/sign-in");
    expect(normalizePathPrefix("/sign-in?next=/")).toBe("/sign-in");
    expect(normalizePathPrefix("/sign-in#main")).toBe("/sign-in");
    expect(normalizePathPrefix("/")).toBe("/");
  });
});

describe("isMigratedPath", () => {
  it("matches a prefix exactly and below it, but never a longer sibling segment", () => {
    const prefixes = ["/sign-in", "/god-mode/users"];
    expect(isMigratedPath("/sign-in", prefixes)).toBe(true);
    expect(isMigratedPath("/sign-in/", prefixes)).toBe(true);
    expect(isMigratedPath("/sign-in?next=/", prefixes)).toBe(true);
    expect(isMigratedPath("/god-mode/users/42", prefixes)).toBe(true);
    expect(isMigratedPath("/sign-invites", prefixes)).toBe(false);
    expect(isMigratedPath("/god-mode", prefixes)).toBe(false);
    expect(isMigratedPath("/", prefixes)).toBe(false);
  });

  it("ignores blank entries and treats / as everything", () => {
    expect(isMigratedPath("/anything", ["", "   "])).toBe(false);
    expect(isMigratedPath("/anything/deep", ["/"])).toBe(true);
    expect(isMigratedPath("/", ["/"])).toBe(true);
  });

  it("defaults to the bundled list, which starts empty", () => {
    expect(MIGRATED_ROUTE_PREFIXES).toEqual([]);
    expect(isMigratedPath("/sign-in")).toBe(false);
  });
});

describe("resolveLinkKind", () => {
  it("sends migrated paths to the router and everything else to a plain anchor", () => {
    const prefixes = ["/sign-in"];
    expect(resolveLinkKind("/sign-in", prefixes)).toBe("router");
    expect(resolveLinkKind("/sign-in/", prefixes)).toBe("router");
    expect(resolveLinkKind("/projects/1/issues", prefixes)).toBe("anchor");
    expect(resolveLinkKind("/projects/1/issues")).toBe("anchor");
  });
});
