// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles list presentations (CYC-048): the two menu entries with
// their pinned labels, the parse fallback, and the label lookup.

import { describe, expect, it } from "vitest";

import {
  CYCLE_LIST_LAYOUT_OPTIONS,
  CYCLE_LIST_LAYOUTS,
  DEFAULT_CYCLE_LIST_LAYOUT,
  cycleListLayoutLabel,
  isCycleListLayout,
  parseCycleListLayout,
} from "./cycleListLayout.js";

describe("cycleListLayout", () => {
  it("offers exactly the list and gallery presentations in order", () => {
    expect([...CYCLE_LIST_LAYOUTS]).toEqual(["list", "gallery"]);
    expect(CYCLE_LIST_LAYOUT_OPTIONS.map((option) => option.label)).toEqual(["List layout", "Gallery layout"]);
    expect(CYCLE_LIST_LAYOUT_OPTIONS.map((option) => option.layout)).toEqual(["list", "gallery"]);
  });

  it("defaults to the list presentation", () => {
    expect(DEFAULT_CYCLE_LIST_LAYOUT).toBe("list");
  });

  it("recognizes only the two known keys", () => {
    expect(isCycleListLayout("list")).toBe(true);
    expect(isCycleListLayout("gallery")).toBe(true);
    expect(isCycleListLayout("timeline")).toBe(false);
    expect(isCycleListLayout(null)).toBe(false);
  });

  it("falls back to the list presentation for unknown values", () => {
    expect(parseCycleListLayout("gallery")).toBe("gallery");
    expect(parseCycleListLayout("timeline")).toBe("list");
    expect(parseCycleListLayout(undefined)).toBe("list");
  });

  it("labels each presentation for the menu trigger", () => {
    expect(cycleListLayoutLabel("list")).toBe("List layout");
    expect(cycleListLayoutLabel("gallery")).toBe("Gallery layout");
  });
});
