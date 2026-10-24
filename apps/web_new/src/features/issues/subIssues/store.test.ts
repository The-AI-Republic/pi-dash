// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-parent widget state (parity/specs/issues.md §15, ISS-173/178): the
// open flag, filters, display settings and expanded rows stay with one
// parent, and sign-out clears the whole map.
import { beforeEach, describe, expect, it } from "vitest";

import { resetRegisteredStores } from "../../../core/session/index.js";
import { createParentState, selectParentSubIssueState, useSubIssuesUiStore } from "./store.js";

const A = "123e4567-e89b-12d3-a456-426614174001";
const B = "223e4567-e89b-12d3-a456-426614174002";

function reset(): void {
  useSubIssuesUiStore.setState({ parents: {} });
}

describe("createParentState", () => {
  it("opens by default with fresh filters and display objects", () => {
    const one = createParentState();
    const two = createParentState();
    expect(one.open).toBe(true);
    expect(one.filters.priority).toEqual([]);
    expect(one.display.orderBy).toBe("-created_at");
    expect(one.filters).not.toBe(two.filters);
    expect(one.display.properties).not.toBe(two.display.properties);
  });
});

describe("sub-issues UI store", () => {
  beforeEach(reset);

  it("returns a blank record for an unseen parent without storing one", () => {
    const seen = selectParentSubIssueState(A)(useSubIssuesUiStore.getState());
    expect(seen.open).toBe(true);
    expect(useSubIssuesUiStore.getState().parents[A]).toBeUndefined();
  });

  it("keeps one parent's open flag apart from another's", () => {
    useSubIssuesUiStore.getState().setOpen(A, false);
    expect(useSubIssuesUiStore.getState().stateFor(A).open).toBe(false);
    expect(useSubIssuesUiStore.getState().stateFor(B).open).toBe(true);
    useSubIssuesUiStore.getState().toggleOpen(A);
    expect(useSubIssuesUiStore.getState().stateFor(A).open).toBe(true);
    useSubIssuesUiStore.getState().toggleOpen(B);
    expect(useSubIssuesUiStore.getState().stateFor(B).open).toBe(false);
  });

  it("toggles and replaces filters per dimension", () => {
    const store = useSubIssuesUiStore.getState();
    store.toggleFilter(A, "priority", "high");
    store.toggleFilter(A, "priority", "urgent");
    expect(useSubIssuesUiStore.getState().stateFor(A).filters.priority).toEqual(["high", "urgent"]);
    store.toggleFilter(A, "priority", "high");
    expect(useSubIssuesUiStore.getState().stateFor(A).filters.priority).toEqual(["urgent"]);
    store.setFilterValues(A, "start_date", ["2026-10-01;after"]);
    expect(useSubIssuesUiStore.getState().stateFor(A).filters.start_date).toEqual(["2026-10-01;after"]);
    expect(useSubIssuesUiStore.getState().stateFor(A).filters.priority).toEqual(["urgent"]);
    store.resetFilters(A);
    expect(useSubIssuesUiStore.getState().stateFor(A).filters.priority).toEqual([]);
    expect(useSubIssuesUiStore.getState().stateFor(A).filters.start_date).toEqual([]);
  });

  it("keeps the edition dimensions in the same per-parent record", () => {
    const store = useSubIssuesUiStore.getState();
    store.toggleEditionFilter(A, "teamProject", "team-a");
    store.toggleEditionFilter(A, "issueType", "type-a");
    store.toggleEditionFilter(A, "teamProject", "team-b");
    expect(useSubIssuesUiStore.getState().stateFor(A).editionFilters).toEqual({
      teamProject: ["team-a", "team-b"],
      issueType: ["type-a"],
    });
    expect(useSubIssuesUiStore.getState().stateFor(B).editionFilters).toEqual({ teamProject: [], issueType: [] });
    store.toggleEditionFilter(A, "teamProject", "team-a");
    expect(useSubIssuesUiStore.getState().stateFor(A).editionFilters.teamProject).toEqual(["team-b"]);
  });

  it("clears the edition dimensions with the base filters", () => {
    const store = useSubIssuesUiStore.getState();
    store.toggleFilter(A, "priority", "high");
    store.toggleEditionFilter(A, "issueType", "type-a");
    store.resetFilters(A);
    const stateA = useSubIssuesUiStore.getState().stateFor(A);
    expect(stateA.filters.priority).toEqual([]);
    expect(stateA.editionFilters).toEqual({ teamProject: [], issueType: [] });
  });

  it("keeps grouping, ordering and property choices per parent", () => {
    const store = useSubIssuesUiStore.getState();
    store.setGroupBy(A, "state");
    store.setOrderBy(A, "-priority");
    store.toggleProperty(A, "assignee");
    const stateA = useSubIssuesUiStore.getState().stateFor(A);
    expect(stateA.display.groupBy).toBe("state");
    expect(stateA.display.orderBy).toBe("-priority");
    expect(stateA.display.properties.assignee).toBe(false);
    expect(useSubIssuesUiStore.getState().stateFor(B).display.groupBy).toBeNull();
    store.toggleProperty(A, "assignee");
    expect(useSubIssuesUiStore.getState().stateFor(A).display.properties.assignee).toBe(true);
  });

  it("clears every parent on sign-out", () => {
    useSubIssuesUiStore.getState().setOpen(A, false);
    expect(useSubIssuesUiStore.getState().parents[A]).toBeDefined();
    resetRegisteredStores();
    expect(useSubIssuesUiStore.getState().parents).toEqual({});
  });
});
