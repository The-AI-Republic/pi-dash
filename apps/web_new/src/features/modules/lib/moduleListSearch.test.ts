// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The modules list's validated search (parity/specs/modules.md §5,
// MOD-019): `peekModule` is validated and normalised, a malformed value is
// dropped at the route boundary, and every other key survives so a shared
// link keeps its parameters through an open or a close.

import { describe, expect, it } from "vitest";

import { MODULES_MODULE_ID } from "../testing.js";
import { parseModulesListSearch } from "./moduleListSearch.js";

describe("parseModulesListSearch", () => {
  it("keeps a valid peek id", () => {
    expect(parseModulesListSearch({ peekModule: MODULES_MODULE_ID })).toEqual({ peekModule: MODULES_MODULE_ID });
  });

  it("drops a value that is not a module id", () => {
    expect(parseModulesListSearch({ peekModule: "Delivery" })).toEqual({});
    expect(parseModulesListSearch({ peekModule: 42 })).toEqual({});
  });

  it("reads an empty search as no peek", () => {
    expect(parseModulesListSearch({})).toEqual({});
  });

  it("keeps a valid peek next to the keys it does not interpret", () => {
    expect(parseModulesListSearch({ peekModule: MODULES_MODULE_ID, layout: "board", q: "del" })).toEqual({
      peekModule: MODULES_MODULE_ID,
      layout: "board",
      q: "del",
    });
  });

  it("keeps the other keys and drops only a malformed peek", () => {
    expect(parseModulesListSearch({ peekModule: "Delivery", layout: "board" })).toEqual({ layout: "board" });
  });

  it("keeps the other keys when the address carries no peek at all", () => {
    expect(parseModulesListSearch({ layout: "board" })).toEqual({ layout: "board" });
  });
});
