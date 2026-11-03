// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The three layout keys, their order and labels, the compact menu's two
// pinned labels, and the stored-value fallbacks (MOD-011).

import { describe, expect, it } from "vitest";

import {
  DEFAULT_MODULES_LAYOUT,
  isModulesLayout,
  MODULES_COMPACT_LAYOUT_OPTIONS,
  MODULES_LAYOUT_LABELS,
  MODULES_LAYOUTS,
  parseModulesLayout,
  resolveCompactLayout,
} from "./moduleLayouts.js";

describe("module layouts", () => {
  it("offers list, board and timeline in switcher order", () => {
    expect(MODULES_LAYOUTS).toEqual(["list", "board", "timeline"]);
    expect(DEFAULT_MODULES_LAYOUT).toBe("list");
    expect(MODULES_LAYOUT_LABELS).toEqual({ list: "List", board: "Board", timeline: "Timeline" });
  });

  it("offers exactly two compact options with the pinned labels", () => {
    expect(MODULES_COMPACT_LAYOUT_OPTIONS).toEqual([
      { layout: "list", label: "List layout" },
      { layout: "board", label: "Gallery layout" },
    ]);
  });

  it("recognizes only the three layout keys", () => {
    expect(isModulesLayout("list")).toBe(true);
    expect(isModulesLayout("board")).toBe(true);
    expect(isModulesLayout("timeline")).toBe(true);
    expect(isModulesLayout("kanban")).toBe(false);
    expect(isModulesLayout("gantt_chart")).toBe(false);
    expect(isModulesLayout(null)).toBe(false);
    expect(isModulesLayout(undefined)).toBe(false);
    expect(isModulesLayout(3)).toBe(false);
  });

  it("falls back to list for anything unknown", () => {
    expect(parseModulesLayout("board")).toBe("board");
    expect(parseModulesLayout("bogus")).toBe("list");
    expect(parseModulesLayout(undefined)).toBe("list");
  });

  it("maps the stored layout onto what a small screen may render", () => {
    expect(resolveCompactLayout("list")).toBe("list");
    expect(resolveCompactLayout("board")).toBe("board");
    expect(resolveCompactLayout("timeline")).toBe("list");
  });
});
