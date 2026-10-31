// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The peek address rules (parity/specs/modules.md §5, MOD-019): opening
// writes the key, closing removes only that key, an unusable id opens
// nothing, and the board grid narrows one wide column while the panel is
// open.

import { describe, expect, it } from "vitest";

import { MODULES_MODULE_ID } from "../testing.js";
import { moduleBoardGridClass, modulePeekFromSearch, withModulePeek } from "./modulePeek.js";

describe("withModulePeek", () => {
  it("adds the key and keeps every other search entry", () => {
    expect(withModulePeek({ layout: "board", q: "del" }, MODULES_MODULE_ID)).toEqual({
      layout: "board",
      q: "del",
      peekModule: MODULES_MODULE_ID,
    });
  });

  it("replaces an existing peek without mutating the input", () => {
    const search = { peekModule: "11111111-1111-4111-8111-111111111111" };
    const next = withModulePeek(search, MODULES_MODULE_ID);
    expect(next).toEqual({ peekModule: MODULES_MODULE_ID });
    expect(search).toEqual({ peekModule: "11111111-1111-4111-8111-111111111111" });
  });

  it("removes only the peek key when closing", () => {
    expect(withModulePeek({ layout: "list", peekModule: MODULES_MODULE_ID, q: "x" }, null)).toEqual({
      layout: "list",
      q: "x",
    });
  });

  it("closing a search without the key leaves it as it was", () => {
    expect(withModulePeek({ layout: "list" }, null)).toEqual({ layout: "list" });
  });

  it("refuses a value that is not a module id", () => {
    expect(withModulePeek({ layout: "list", peekModule: MODULES_MODULE_ID }, "not-an-id")).toEqual({
      layout: "list",
    });
  });
});

describe("modulePeekFromSearch", () => {
  it("reads a module id", () => {
    expect(modulePeekFromSearch({ peekModule: MODULES_MODULE_ID })).toBe(MODULES_MODULE_ID);
  });

  it("reads absent, non-string and malformed values as no peek", () => {
    expect(modulePeekFromSearch({})).toBeNull();
    expect(modulePeekFromSearch({ peekModule: 42 })).toBeNull();
    expect(modulePeekFromSearch({ peekModule: "Delivery" })).toBeNull();
  });
});

describe("moduleBoardGridClass", () => {
  it("uses three wide columns while the peek is closed", () => {
    expect(moduleBoardGridClass({ peekOpen: false })).toContain("xl:grid-cols-3");
    expect(moduleBoardGridClass({ peekOpen: false })).not.toContain("xl:grid-cols-2");
  });

  it("drops to two wide columns while the peek is open", () => {
    expect(moduleBoardGridClass({ peekOpen: true })).toContain("xl:grid-cols-2");
    expect(moduleBoardGridClass({ peekOpen: true })).not.toContain("xl:grid-cols-3");
  });

  it("keeps the narrow bands in both states", () => {
    for (const peekOpen of [true, false]) {
      const classes = moduleBoardGridClass({ peekOpen });
      expect(classes).toContain("grid-cols-1");
      expect(classes).toContain("md:grid-cols-2");
    }
  });
});
