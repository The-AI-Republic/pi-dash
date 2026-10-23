// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  isValidAnchor,
  legacyForwardHref,
  normalizeBoardHref,
  parseSpaceBoardSearch,
  spaceBoardPath,
  spaceLegacyPath,
  spaceReturnPath,
} from "./address.js";

describe("space address rules", () => {
  it("accepts only the documented anchor alphabet", () => {
    expect(isValidAnchor("abc123")).toBe(true);
    expect(isValidAnchor("a_b-C9")).toBe(true);
    expect(isValidAnchor("")).toBe(false);
    expect(isValidAnchor("!!!")).toBe(false);
    expect(isValidAnchor("a/b")).toBe(false);
    expect(isValidAnchor("a b")).toBe(false);
  });

  it("builds canonical board and legacy addresses", () => {
    expect(spaceBoardPath("anchor-1")).toBe("/spaces/issues/anchor-1/");
    expect(spaceLegacyPath("acme", "project-1")).toBe("/spaces/acme/project-1/");
  });

  it("keeps only valid board and peek parameters", () => {
    expect(parseSpaceBoardSearch({ board: "kanban", peekId: "issue-1", labels: "x" })).toEqual({
      board: "kanban",
      peekId: "issue-1",
    });
    expect(parseSpaceBoardSearch({ board: "timeline", peekId: "" })).toEqual({});
    expect(parseSpaceBoardSearch({ board: 42, peekId: null })).toEqual({});
  });

  it("normalizes a settled board address with a stable parameter order", () => {
    expect(normalizeBoardHref("anchor", { peekId: "p1", board: "list" })).toBe(
      "/spaces/issues/anchor/?board=list&peekId=p1"
    );
    expect(normalizeBoardHref("anchor", {})).toBe("/spaces/issues/anchor/");
  });

  it("forwards exactly board and peekId from a legacy link", () => {
    expect(legacyForwardHref("anchor", { board: "kanban", peekId: "p1", state: "s1", priority: "high" })).toBe(
      "/spaces/issues/anchor/?board=kanban&peekId=p1"
    );
    expect(legacyForwardHref("anchor", { board: "bogus", labels: "l1" })).toBe("/spaces/issues/anchor/");
  });

  it("carries a subtree-relative return address without the mount segment", () => {
    expect(spaceReturnPath("anchor", { board: "kanban" })).toBe("/issues/anchor/?board=kanban");
    expect(spaceReturnPath("anchor", {})).toBe("/issues/anchor/");
  });
});

describe("space sign-in return address", () => {
  it("spells the landing mount while keeping path slashes literal", async () => {
    const { spaceSignInHref } = await import("./address.js");
    expect(spaceSignInHref("anchor", { board: "list" })).toBe("/spaces/?next_path=/issues/anchor/%3Fboard%3Dlist");
    expect(spaceSignInHref("anchor", {})).toBe("/spaces/?next_path=/issues/anchor/");
  });
});
