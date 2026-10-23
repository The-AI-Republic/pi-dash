// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  isValidAnchor,
  legacyForwardHref,
  normalizeBoardHref,
  parseSpaceBoardSearch,
  SPACE_LAYOUTS,
  SPACE_PRIORITIES,
  spaceBoardPath,
  spaceLegacyPath,
  spaceReturnPath,
} from "./address.js";

const ID_A = "11111111-1111-4111-8111-111111111111";
const ID_B = "22222222-2222-4222-8222-222222222222";

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

describe("board filters in the address", () => {
  it("offers only the two renderable layouts and the five priority levels", () => {
    expect(SPACE_LAYOUTS).toEqual(["list", "kanban"]);
    expect(SPACE_PRIORITIES).toEqual(["urgent", "high", "medium", "low", "none"]);
  });

  it("keeps valid id lists, known priorities and drops anything malformed", () => {
    expect(
      parseSpaceBoardSearch({
        state: `${ID_A},${ID_B}`,
        labels: ID_A,
        priority: "high,low,unset",
      })
    ).toEqual({ state: [ID_A, ID_B], labels: [ID_A], priority: ["high", "low"] });
    expect(parseSpaceBoardSearch({ state: "nope", labels: " , ", priority: "critical" })).toEqual({});
    expect(parseSpaceBoardSearch({ state: ["x"], priority: 3 })).toEqual({});
  });

  it("keeps a filter that arrives with a valid and an invalid value", () => {
    expect(parseSpaceBoardSearch({ state: `${ID_A} , not-an-id` })).toEqual({ state: [ID_A] });
  });

  it("writes a settled address with board first, then peek, priority, state and labels", () => {
    expect(
      normalizeBoardHref("anchor", {
        board: "kanban",
        peekId: "p1",
        state: [ID_A],
        priority: ["urgent"],
        labels: [ID_B],
      })
    ).toBe(`/spaces/issues/anchor/?board=kanban&peekId=p1&priority=urgent&state=${ID_A}&labels=${ID_B}`);
  });

  it("omits empty facets and leaves an untouched address bare", () => {
    expect(normalizeBoardHref("anchor", { state: [], priority: [] })).toBe("/spaces/issues/anchor/");
    expect(normalizeBoardHref("anchor", { labels: [ID_A] })).toBe(`/spaces/issues/anchor/?labels=${ID_A}`);
  });

  it("round-trips a canonical address through the parser unchanged", () => {
    const href = normalizeBoardHref("anchor", { board: "list", state: [ID_A], priority: ["low"] });
    const query = Object.fromEntries(new URL(href, "https://x.test").searchParams);
    expect(normalizeBoardHref("anchor", parseSpaceBoardSearch(query))).toBe(href);
  });

  it("keeps filters in the sign-in return address", () => {
    expect(spaceReturnPath("anchor", { board: "kanban", state: [ID_A] })).toBe(
      `/issues/anchor/?board=kanban&state=${ID_A}`
    );
  });
});

describe("space sign-in return address", () => {
  it("spells the landing mount while keeping path slashes literal", async () => {
    const { spaceSignInHref } = await import("./address.js");
    expect(spaceSignInHref("anchor", { board: "list" })).toBe("/spaces/?next_path=/issues/anchor/%3Fboard%3Dlist");
    expect(spaceSignInHref("anchor", {})).toBe("/spaces/?next_path=/issues/anchor/");
  });
});
