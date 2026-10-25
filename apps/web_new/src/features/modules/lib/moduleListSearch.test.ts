// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The modules list's validated search (parity/specs/modules.md §5,
// MOD-019): only `peekModule` lives in the address, and a malformed value
// is dropped at the route boundary.

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

  it("drops keys the screen does not own", () => {
    expect(parseModulesListSearch({ peekModule: MODULES_MODULE_ID, layout: "board" })).toEqual({
      peekModule: MODULES_MODULE_ID,
    });
  });
});
