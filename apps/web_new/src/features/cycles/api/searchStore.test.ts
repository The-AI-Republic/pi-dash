// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Remembered queries: one per project, cleared per project, reset whole.

import { beforeEach, describe, expect, it } from "vitest";

import { resetCycleSearchStore, useCycleSearchStore } from "./searchStore.js";

beforeEach(() => {
  resetCycleSearchStore();
});

describe("cycle search store", () => {
  it("starts with an empty query per project", () => {
    expect(useCycleSearchStore.getState().queryFor("p1")).toBe("");
  });

  it("remembers one query per project", () => {
    useCycleSearchStore.getState().setQuery("p1", "alpha");
    useCycleSearchStore.getState().setQuery("p2", "beta");
    expect(useCycleSearchStore.getState().queryFor("p1")).toBe("alpha");
    expect(useCycleSearchStore.getState().queryFor("p2")).toBe("beta");
  });

  it("clears one project's query and leaves the others", () => {
    useCycleSearchStore.getState().setQuery("p1", "alpha");
    useCycleSearchStore.getState().setQuery("p2", "beta");
    useCycleSearchStore.getState().clearQuery("p1");
    expect(useCycleSearchStore.getState().queryFor("p1")).toBe("");
    expect(useCycleSearchStore.getState().queryFor("p2")).toBe("beta");
  });

  it("clearing an unremembered project is a no-op", () => {
    const before = useCycleSearchStore.getState();
    useCycleSearchStore.getState().clearQuery("p9");
    expect(useCycleSearchStore.getState()).toBe(before);
  });

  it("resets every query at once", () => {
    useCycleSearchStore.getState().setQuery("p1", "alpha");
    resetCycleSearchStore();
    expect(useCycleSearchStore.getState().queryFor("p1")).toBe("");
  });
});
