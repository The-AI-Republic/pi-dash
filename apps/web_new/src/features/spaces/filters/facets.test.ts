// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  clearedFilterSearch,
  clearFilterFacet,
  facetValues,
  filteredBoardSearch,
  hasAppliedFilters,
  optionMatchesQuery,
  panelFacetsForLayout,
  previewStateOptions,
  removeFilterValue,
  SPACE_FACET_MESSAGE_KEYS,
  SPACE_PRIORITY_MESSAGE_KEYS,
  SPACE_STATE_OPTION_PREVIEW,
  SPACE_STRIP_FACET_ORDER,
  toggleFilterValue,
} from "./facets.js";

const STATE_A = "11111111-1111-4111-8111-111111111111";
const STATE_B = "22222222-2222-4222-8222-222222222222";
const LABEL = "33333333-3333-4333-8333-333333333333";

describe("panelFacetsForLayout (SPC-038)", () => {
  it("offers priority and state on both public layouts", () => {
    expect(panelFacetsForLayout("list")).toEqual(["priority", "state"]);
    expect(panelFacetsForLayout("kanban")).toEqual(["priority", "state"]);
  });

  it("renders strip shells in priority, state, labels order", () => {
    expect(SPACE_STRIP_FACET_ORDER).toEqual(["priority", "state", "labels"]);
    expect(SPACE_FACET_MESSAGE_KEYS).toEqual({
      priority: "filters.priority",
      state: "filters.state",
      labels: "filters.labels",
    });
    expect(SPACE_PRIORITY_MESSAGE_KEYS).toEqual({
      urgent: "filters.priority.urgent",
      high: "filters.priority.high",
      medium: "filters.priority.medium",
      low: "filters.priority.low",
      none: "filters.priority.none",
    });
  });
});

describe("facetValues / hasAppliedFilters", () => {
  it("reads applied values and reports an empty filter object as clear", () => {
    expect(facetValues({}, "priority")).toEqual([]);
    expect(facetValues({ priority: ["high"] }, "priority")).toEqual(["high"]);
    expect(hasAppliedFilters({})).toBe(false);
    expect(hasAppliedFilters({ state: [], priority: [], labels: [] })).toBe(false);
  });

  it("reports each facet on its own", () => {
    expect(hasAppliedFilters({ priority: ["high"] })).toBe(true);
    expect(hasAppliedFilters({ state: [STATE_A] })).toBe(true);
    expect(hasAppliedFilters({ labels: [LABEL] })).toBe(true);
  });
});

describe("toggleFilterValue (SPC-034)", () => {
  it("adds a value to a clear facet", () => {
    expect(toggleFilterValue({}, "priority", "high")).toEqual({ priority: ["high"] });
  });

  it("appends to surviving values without touching other facets", () => {
    expect(toggleFilterValue({ priority: ["high"], state: [STATE_A] }, "priority", "urgent")).toEqual({
      priority: ["high", "urgent"],
      state: [STATE_A],
    });
  });

  it("drops an applied value and removes the facet key when nothing survives", () => {
    expect(toggleFilterValue({ priority: ["high", "urgent"] }, "priority", "high")).toEqual({
      priority: ["urgent"],
    });
    expect(toggleFilterValue({ priority: ["high"], labels: [LABEL] }, "priority", "high")).toEqual({
      labels: [LABEL],
    });
  });
});

describe("removeFilterValue (SPC-035)", () => {
  it("drops one value and keeps the survivors under the same facet", () => {
    expect(removeFilterValue({ state: [STATE_A, STATE_B] }, "state", STATE_A)).toEqual({ state: [STATE_B] });
  });

  it("removes the facet key when the last value goes", () => {
    expect(removeFilterValue({ labels: [LABEL], priority: ["low"] }, "labels", LABEL)).toEqual({
      priority: ["low"],
    });
  });

  it("leaves the filters alone when the value is not applied", () => {
    const filters = { priority: ["high"] };
    expect(removeFilterValue(filters, "priority", "urgent")).toBe(filters);
    expect(removeFilterValue(filters, "state", STATE_A)).toBe(filters);
  });
});

describe("clearFilterFacet (SPC-035)", () => {
  it("drops one facet and keeps the others", () => {
    expect(clearFilterFacet({ priority: ["high"], state: [STATE_A] }, "priority")).toEqual({ state: [STATE_A] });
  });

  it("is a no-op shape when the facet is already clear", () => {
    expect(clearFilterFacet({ priority: ["high"] }, "state")).toEqual({ priority: ["high"] });
  });
});

describe("clearedFilterSearch / filteredBoardSearch (SPC-036)", () => {
  it("clear-all keeps the view and the open issue and drops every facet", () => {
    expect(
      clearedFilterSearch({ board: "kanban", peekId: "p1", priority: ["high"], state: [STATE_A], labels: [LABEL] })
    ).toEqual({ board: "kanban", peekId: "p1" });
    expect(clearedFilterSearch({ priority: ["high"] })).toEqual({});
  });

  it("rebuilds the search from a view plus a filter object", () => {
    expect(
      filteredBoardSearch({ board: "list", peekId: "p1" }, { priority: ["high", "urgent"], labels: [LABEL] })
    ).toEqual({ board: "list", peekId: "p1", priority: ["high", "urgent"], labels: [LABEL] });
    expect(filteredBoardSearch({}, {})).toEqual({});
  });
});

describe("optionMatchesQuery (SPC-039)", () => {
  it("matches everything on an empty query", () => {
    expect(optionMatchesQuery(["High"], "")).toBe(true);
    expect(optionMatchesQuery(["High"], "   ")).toBe(true);
  });

  it("matches case-insensitively on substrings across every candidate", () => {
    expect(optionMatchesQuery(["High", "high"], "HIG")).toBe(true);
    expect(optionMatchesQuery(["High", "high"], "igh")).toBe(true);
    expect(optionMatchesQuery(["Todo"], "hig")).toBe(false);
    expect(optionMatchesQuery(["Urgent"], "high")).toBe(false);
  });
});

describe("previewStateOptions (SPC-034)", () => {
  it("caps the preview at five and shows everything once expanded", () => {
    const options = ["a", "b", "c", "d", "e", "f", "g"];
    expect(SPACE_STATE_OPTION_PREVIEW).toBe(5);
    expect(previewStateOptions(options, false)).toEqual(["a", "b", "c", "d", "e"]);
    expect(previewStateOptions(options, true)).toEqual(options);
    expect(previewStateOptions(["a"], false)).toEqual(["a"]);
  });
});
