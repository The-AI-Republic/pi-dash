// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { beforeEach, describe, expect, it } from "vitest";

import { resetRegisteredStores } from "../../../core/session/store.js";
import { registerListDisplayReset, useListDisplayStore } from "./list-display.js";

// Mirror the production wiring: the list view registers this store once
// from a mount effect (see ProjectsListView), so the suite does the same.
registerListDisplayReset();

beforeEach(() => {
  useListDisplayStore.setState({ displays: {}, searchQuery: "", activeWorkspace: null });
});

describe("useListDisplayStore", () => {
  it("defaults an untouched workspace to creation order without filters", () => {
    const display = useListDisplayStore.getState().displayFor("acme");
    expect(display).toEqual({
      sort: { key: "created", direction: "ascending" },
      filters: { myProjects: false, access: [], lead: [], members: [], createdAt: [] },
    });
    expect(useListDisplayStore.getState().displays["acme"]).toBeUndefined();
  });

  it("keeps each workspace's sort apart", () => {
    const { setSort, displayFor } = useListDisplayStore.getState();
    setSort("acme", { key: "name", direction: "descending" });
    expect(displayFor("acme").sort).toEqual({ key: "name", direction: "descending" });
    expect(displayFor("globex").sort).toEqual({ key: "created", direction: "ascending" });
  });

  it("toggles My projects without touching the sort", () => {
    const { setMyProjects, setSort, displayFor } = useListDisplayStore.getState();
    setSort("acme", { key: "name", direction: "ascending" });
    setMyProjects("acme", true);
    expect(displayFor("acme").filters.myProjects).toBe(true);
    expect(displayFor("acme").sort.key).toBe("name");
    setMyProjects("acme", false);
    expect(displayFor("acme").filters.myProjects).toBe(false);
  });

  it("toggles access values both ways", () => {
    const { toggleAccess, displayFor } = useListDisplayStore.getState();
    toggleAccess("acme", 0);
    toggleAccess("acme", 2);
    expect(displayFor("acme").filters.access).toEqual([0, 2]);
    toggleAccess("acme", 0);
    expect(displayFor("acme").filters.access).toEqual([2]);
  });

  it("toggles people per dimension", () => {
    const { togglePerson, displayFor } = useListDisplayStore.getState();
    togglePerson("acme", "lead", "u1");
    togglePerson("acme", "members", "u1");
    expect(displayFor("acme").filters.lead).toEqual(["u1"]);
    expect(displayFor("acme").filters.members).toEqual(["u1"]);
    togglePerson("acme", "lead", "u1");
    expect(displayFor("acme").filters.lead).toEqual([]);
    expect(displayFor("acme").filters.members).toEqual(["u1"]);
  });

  it("replaces creation dates on every pick", () => {
    const { selectCreated, displayFor } = useListDisplayStore.getState();
    selectCreated("acme", ["today;custom;custom"]);
    expect(displayFor("acme").filters.createdAt).toEqual(["today;custom;custom"]);
    selectCreated("acme", ["2026-10-01;after", "2026-10-31;before"]);
    expect(displayFor("acme").filters.createdAt).toEqual(["2026-10-01;after", "2026-10-31;before"]);
    selectCreated("acme", []);
    expect(displayFor("acme").filters.createdAt).toEqual([]);
  });

  it("removes one filter value per dimension", () => {
    const { toggleAccess, togglePerson, selectCreated, removeFilterValue, displayFor } = useListDisplayStore.getState();
    toggleAccess("acme", 0);
    togglePerson("acme", "lead", "u1");
    selectCreated("acme", ["2026-10-01;after", "2026-10-31;before"]);
    removeFilterValue("acme", "access", "0");
    removeFilterValue("acme", "lead", "u1");
    removeFilterValue("acme", "createdAt", "2026-10-01;after");
    const filters = displayFor("acme").filters;
    expect(filters.access).toEqual([]);
    expect(filters.lead).toEqual([]);
    expect(filters.createdAt).toEqual(["2026-10-31;before"]);
  });

  it("removes whole dimensions", () => {
    const { toggleAccess, togglePerson, removeFilterDimension, displayFor } = useListDisplayStore.getState();
    toggleAccess("acme", 0);
    toggleAccess("acme", 2);
    togglePerson("acme", "members", "u1");
    removeFilterDimension("acme", "access");
    removeFilterDimension("acme", "members");
    const filters = displayFor("acme").filters;
    expect(filters.access).toEqual([]);
    expect(filters.members).toEqual([]);
  });

  it("clears filters while keeping the sort", () => {
    const { toggleAccess, setMyProjects, setSort, clearListFilters, displayFor } = useListDisplayStore.getState();
    setSort("acme", { key: "name", direction: "descending" });
    toggleAccess("acme", 0);
    setMyProjects("acme", true);
    clearListFilters("acme");
    expect(displayFor("acme")).toEqual({
      sort: { key: "name", direction: "descending" },
      filters: { myProjects: false, access: [], lead: [], members: [], createdAt: [] },
    });
  });

  it("holds the search text and drops it on workspace switches", () => {
    const { setSearchQuery, trackWorkspace } = useListDisplayStore.getState();
    trackWorkspace("acme");
    setSearchQuery("apple");
    expect(useListDisplayStore.getState().searchQuery).toBe("apple");
    trackWorkspace("acme");
    expect(useListDisplayStore.getState().searchQuery).toBe("apple");
    trackWorkspace("globex");
    expect(useListDisplayStore.getState().searchQuery).toBe("");
  });

  it("resets with every other store on sign-out", () => {
    const { setSort, setSearchQuery } = useListDisplayStore.getState();
    setSort("acme", { key: "name", direction: "ascending" });
    setSearchQuery("apple");
    resetRegisteredStores();
    const state = useListDisplayStore.getState();
    expect(state.displays).toEqual({});
    expect(state.searchQuery).toBe("");
    expect(state.activeWorkspace).toBeNull();
  });
});
