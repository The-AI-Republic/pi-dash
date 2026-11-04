// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cross-project move rules (PAGE-017, PDASHOSS01-347): the admin-and-unlocked
// gate, the writable-target filter, the server reason reader and the
// subtree walk the cache drop relies on.

import { ApiError, type ProjectList } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { makePageRow } from "../testing.js";
import { canMovePageToProject, movedSubtreeIds, pageMoveErrorMessage, pageMoveProjectIds } from "./pageProjectMove.js";

const CURRENT = "11111111-1111-4111-8111-111111111111";

function project(id: string, memberRole: number | null, archivedAt: string | null = null): ProjectList[number] {
  return { id, member_role: memberRole, archived_at: archivedAt } as ProjectList[number];
}

describe("canMovePageToProject", () => {
  it("lets only an admin move an unlocked page", () => {
    expect(canMovePageToProject({ admin: true, locked: false })).toBe(true);
    expect(canMovePageToProject({ admin: true, locked: true })).toBe(false);
    expect(canMovePageToProject({ admin: false, locked: false })).toBe(false);
    expect(canMovePageToProject({ admin: false, locked: true })).toBe(false);
  });
});

describe("pageMoveProjectIds", () => {
  it("keeps other live projects where the viewer is a member or admin, in server order", () => {
    const ids = pageMoveProjectIds(
      [
        project(CURRENT, 20),
        project("a", 20),
        project("b", 15),
        project("c", 5),
        project("d", null),
        project("e", 20, "2026-01-01T00:00:00Z"),
      ],
      CURRENT
    );
    expect(ids).toEqual(["a", "b"]);
  });

  it("is empty when the workspace has nowhere else to go", () => {
    expect(pageMoveProjectIds([project(CURRENT, 20)], CURRENT)).toEqual([]);
    expect(pageMoveProjectIds([], CURRENT)).toEqual([]);
  });
});

describe("pageMoveErrorMessage", () => {
  it("prefers the body's error text over the folded machine code", () => {
    const error = new ApiError({
      status: 400,
      code: "4701",
      message: "PAGE_LOCKED",
      body: { error: "Page is locked", error_code: 4701, error_message: "PAGE_LOCKED" },
    });
    expect(pageMoveErrorMessage(error)).toBe("Page is locked");
  });

  it("reads a permission refusal's detail", () => {
    const error = new ApiError({
      status: 403,
      code: "http",
      message: "You do not have permission to perform this action.",
      body: { detail: "You do not have permission to perform this action." },
    });
    expect(pageMoveErrorMessage(error)).toBe("You do not have permission to perform this action.");
  });

  it("has no reason for transport failures, empty bodies or foreign errors", () => {
    expect(pageMoveErrorMessage(new ApiError({ status: 0, code: "network", message: "offline" }))).toBeUndefined();
    expect(
      pageMoveErrorMessage(new ApiError({ status: 500, code: "http", message: "Request failed" }))
    ).toBeUndefined();
    expect(
      pageMoveErrorMessage(new ApiError({ status: 400, code: "http", message: "x", body: { error: "  " } }))
    ).toBeUndefined();
    expect(pageMoveErrorMessage(new Error("boom"))).toBeUndefined();
    expect(pageMoveErrorMessage(null)).toBeUndefined();
  });
});

describe("movedSubtreeIds", () => {
  it("walks every depth, prefers parent_id, and ignores duplicates and unrelated rows", () => {
    const rows = [
      makePageRow({ id: "root" }),
      makePageRow({ id: "child", parent: "root", parent_id: "root" }),
      makePageRow({ id: "child", parent: "root", parent_id: "root" }),
      makePageRow({ id: "grand", parent: null, parent_id: "child" }),
      makePageRow({ id: "other", parent: null }),
      makePageRow({ id: "other-child", parent: "other" }),
    ];
    expect([...movedSubtreeIds(rows, "root")].sort()).toEqual(["child", "grand", "root"]);
  });

  it("survives a malformed cycle", () => {
    const rows = [makePageRow({ id: "a", parent: "b" }), makePageRow({ id: "b", parent: "a" })];
    expect([...movedSubtreeIds(rows, "a")].sort()).toEqual(["a", "b"]);
  });
});
