// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The archived-cycles body decision: the pending read skeletons, a
// failed read errors, an empty archive zeroes, narrowing to zero
// names search or filters as the cause, and anything left lists.

import { describe, expect, it } from "vitest";

import { resolveArchivedCyclesBody } from "./archivedCyclesBody.js";

describe("resolveArchivedCyclesBody", () => {
  it("skeletons while the list read has no data", () => {
    expect(
      resolveArchivedCyclesBody({ listPending: true, listError: false, totalCount: 0, visibleCount: 0, query: "" })
    ).toBe("skeleton");
  });

  it("errors once a settled read fails", () => {
    expect(
      resolveArchivedCyclesBody({ listPending: false, listError: true, totalCount: 0, visibleCount: 0, query: "" })
    ).toBe("error");
  });

  it("zeroes when the archive holds nothing", () => {
    expect(
      resolveArchivedCyclesBody({ listPending: false, listError: false, totalCount: 0, visibleCount: 0, query: "" })
    ).toBe("zero");
  });

  it("names filters when they alone hide every row", () => {
    expect(
      resolveArchivedCyclesBody({ listPending: false, listError: false, totalCount: 2, visibleCount: 0, query: "" })
    ).toBe("noMatchFilters");
  });

  it("names search when it hides every row, alone or combined", () => {
    expect(
      resolveArchivedCyclesBody({
        listPending: false,
        listError: false,
        totalCount: 2,
        visibleCount: 0,
        query: "zzz",
      })
    ).toBe("noMatchSearch");
  });

  it("lists once any row stays visible", () => {
    expect(
      resolveArchivedCyclesBody({ listPending: false, listError: false, totalCount: 2, visibleCount: 1, query: "" })
    ).toBe("list");
  });
});
