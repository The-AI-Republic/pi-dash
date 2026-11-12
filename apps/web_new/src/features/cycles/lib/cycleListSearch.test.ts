// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// List address keys: the peek key round-trips, unknown and invalid keys
// fall back to absent.

import { describe, expect, it } from "vitest";

import { parseCyclesListSearch } from "./cycleListSearch.js";

describe("cycles list search", () => {
  it("keeps a peek key and drops unknown keys", () => {
    expect(parseCyclesListSearch({ peekCycle: "c1" })).toEqual({ peekCycle: "c1" });
    expect(parseCyclesListSearch({ peekCycle: "c1", layout: "board" })).toEqual({ peekCycle: "c1" });
  });

  it("falls back to absent on missing and invalid input", () => {
    expect(parseCyclesListSearch({})).toEqual({});
    expect(parseCyclesListSearch(undefined)).toEqual({});
    expect(parseCyclesListSearch({ peekCycle: 42 })).toEqual({});
  });
});
