// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The module detail screen model (MOD-029, MOD-038): the tab title shape,
// the switcher pick target, the gone-away predicate, the one
// wall/missing/content split and the pinned badge tooltip in both numbers.

import { ApiError } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  isAccessRefusal,
  isModuleGoneAway,
  moduleBadgeTooltip,
  moduleDetailTitle,
  moduleReadOutcome,
  projectIssuesHref,
  switchModuleTarget,
} from "./moduleDetail.js";

describe("moduleDetailTitle", () => {
  it("reads project followed by module", () => {
    expect(moduleDetailTitle("Acme", "Alpha")).toBe("Acme - Alpha");
  });
});

describe("projectIssuesHref", () => {
  it("points at the project's issues screen", () => {
    expect(projectIssuesHref("acme", "p1")).toBe("/acme/projects/p1/issues");
  });
});

describe("switchModuleTarget", () => {
  it("navigates a fresh pick to its detail address", () => {
    expect(switchModuleTarget("acme", "p1", "m1", "m2")).toBe("/acme/projects/p1/modules/m2");
  });

  it("stays put on a cleared or repeated pick", () => {
    expect(switchModuleTarget("acme", "p1", "m1", null)).toBe(null);
    expect(switchModuleTarget("acme", "p1", "m1", "m1")).toBe(null);
  });
});

describe("isModuleGoneAway", () => {
  it("treats an archived record as gone", () => {
    expect(isModuleGoneAway(null, "2026-01-01T00:00:00Z")).toBe(true);
  });

  it("treats a live 404 as gone", () => {
    const notFound = new ApiError({ status: 404, code: "http", message: "missing" });
    expect(isModuleGoneAway(notFound, null)).toBe(true);
  });

  it("keeps other failures and live records on the page", () => {
    const serverError = new ApiError({ status: 500, code: "http", message: "boom" });
    expect(isModuleGoneAway(serverError, null)).toBe(false);
    expect(isModuleGoneAway(new Error("boom"), null)).toBe(false);
    expect(isModuleGoneAway(null, null)).toBe(false);
    expect(isModuleGoneAway(null, undefined)).toBe(false);
  });
});

describe("moduleBadgeTooltip", () => {
  it("pins the plural tooltip", () => {
    expect(moduleBadgeTooltip(2)).toBe("There are 2 work items in this module");
    expect(moduleBadgeTooltip(0)).toBe("There are 0 work items in this module");
  });

  it("uses the singular noun for one item", () => {
    expect(moduleBadgeTooltip(1)).toBe("There are 1 work item in this module");
  });
});

describe("moduleReadOutcome (MOD-038)", () => {
  function statusError(status: number): ApiError {
    return new ApiError({ status, code: "http", message: "denied" });
  }

  it("shows the content for a resolved live module", () => {
    expect(moduleReadOutcome({ error: null, archivedAt: null, isAdmin: false, isPending: false })).toEqual({
      kind: "content",
    });
  });

  it("calls an archived record served on the live address a miss", () => {
    expect(
      moduleReadOutcome({ error: null, archivedAt: "2026-03-01T00:00:00Z", isAdmin: false, isPending: false })
    ).toEqual({
      kind: "missing",
    });
  });

  it("calls a 404 a miss", () => {
    expect(moduleReadOutcome({ error: statusError(404), archivedAt: null, isAdmin: true, isPending: false })).toEqual({
      kind: "missing",
    });
  });

  it("sends a 409 to the join wall whatever the reader's standing", () => {
    for (const isAdmin of [false, true]) {
      expect(moduleReadOutcome({ error: statusError(409), archivedAt: null, isAdmin, isPending: false })).toEqual({
        kind: "wall",
        wall: "join",
      });
    }
  });

  it("offers a workspace admin the join wall on a private project (403)", () => {
    expect(moduleReadOutcome({ error: statusError(403), archivedAt: null, isAdmin: true, isPending: false })).toEqual({
      kind: "wall",
      wall: "join",
    });
  });

  it("shows a refused non-admin the no-access wall, never the missing state", () => {
    expect(moduleReadOutcome({ error: statusError(403), archivedAt: null, isAdmin: false, isPending: false })).toEqual({
      kind: "wall",
      wall: "no-access",
    });
  });

  it("holds the page while the read is pending, whatever the cache still shows (NEWFRONT-1026)", () => {
    // A remount of an errored query refetches: the state is pending with
    // error null, so the gate must hold instead of rendering the page.
    expect(moduleReadOutcome({ error: null, archivedAt: undefined, isAdmin: false, isPending: true })).toEqual({
      kind: "pending",
    });
    // A refetch of a previously failed key carries the old error while it
    // is in flight; the outcome still holds rather than re-deciding early.
    expect(moduleReadOutcome({ error: statusError(404), archivedAt: null, isAdmin: false, isPending: true })).toEqual({
      kind: "pending",
    });
  });

  it("lets a settled archived record win over a stale pending flag", () => {
    expect(
      moduleReadOutcome({ error: null, archivedAt: "2026-03-01T00:00:00Z", isAdmin: false, isPending: true })
    ).toEqual({ kind: "missing" });
  });

  it("keeps transient failures on the content path", () => {
    expect(
      moduleReadOutcome({ error: new Error("network"), archivedAt: null, isAdmin: false, isPending: false })
    ).toEqual({
      kind: "content",
    });
    expect(moduleReadOutcome({ error: statusError(500), archivedAt: null, isAdmin: false, isPending: false })).toEqual({
      kind: "content",
    });
  });

  it("names the refusal statuses", () => {
    expect([401, 402, 403, 404, 409].map((status) => isAccessRefusal(status))).toEqual([
      false,
      false,
      true,
      false,
      true,
    ]);
  });
});
