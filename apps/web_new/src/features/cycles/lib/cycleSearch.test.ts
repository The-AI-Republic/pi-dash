// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Query matching: part of a name, any case, order kept, empty keeps all.

import { describe, expect, it } from "vitest";

import { filterCyclesByQuery, matchesCycleQuery } from "./cycleSearch.js";

describe("cycle search matching", () => {
  it("matches part of a name case-insensitively", () => {
    expect(matchesCycleQuery("Website relaunch", "site")).toBe(true);
    expect(matchesCycleQuery("Website relaunch", "SITE")).toBe(true);
    expect(matchesCycleQuery("Website relaunch", "orbit")).toBe(false);
  });

  it("matches every name on an empty query", () => {
    expect(matchesCycleQuery("Anything", "")).toBe(true);
  });

  it("narrows rows without reordering them", () => {
    const rows = [{ name: "Alpha" }, { name: "Beta" }, { name: "Alphabet" }];
    expect(filterCyclesByQuery(rows, "alpha")).toEqual([{ name: "Alpha" }, { name: "Alphabet" }]);
    expect(filterCyclesByQuery(rows, "zzz")).toEqual([]);
  });

  it("keeps every row on an empty query", () => {
    const rows = [{ name: "Alpha" }, { name: "Beta" }];
    expect(filterCyclesByQuery(rows, "")).toEqual(rows);
  });
});
