// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import {
  DEFAULT_LAYOUT,
  LAYOUT_LABELS,
  LAYOUT_ORDER,
  MOBILE_LAYOUT_ORDER,
  isLayoutKind,
  mobileOfferedLayouts,
  parseLayoutKind,
  resolveActiveLayout,
} from "./kinds.js";

describe("layout kinds", () => {
  it("orders the five layouts list-first for the switcher", () => {
    expect(LAYOUT_ORDER).toEqual(["list", "kanban", "calendar", "spreadsheet", "gantt_chart"]);
    expect(DEFAULT_LAYOUT).toBe("list");
  });

  it("labels every kind for the dropdown entries", () => {
    expect(LAYOUT_ORDER.map((kind) => LAYOUT_LABELS[kind])).toEqual([
      "List",
      "Board",
      "Calendar",
      "Spreadsheet",
      "Timeline",
    ]);
  });

  it("recognises exactly the five stored keys", () => {
    for (const kind of LAYOUT_ORDER) expect(isLayoutKind(kind)).toBe(true);
    expect(isLayoutKind("board")).toBe(false);
    expect(isLayoutKind("gantt")).toBe(false);
    expect(isLayoutKind(null)).toBe(false);
    expect(isLayoutKind(undefined)).toBe(false);
    expect(isLayoutKind(42)).toBe(false);
  });

  it("parses stored values with a list fallback", () => {
    expect(parseLayoutKind("kanban")).toBe("kanban");
    expect(parseLayoutKind("board")).toBe("list");
    expect(parseLayoutKind(null)).toBe("list");
    expect(parseLayoutKind(undefined)).toBe("list");
  });

  it("offers the registered layouts in mobile order", () => {
    expect(MOBILE_LAYOUT_ORDER).toEqual(["list", "kanban", "calendar"]);
    expect(mobileOfferedLayouts(LAYOUT_ORDER)).toEqual(["list", "kanban", "calendar"]);
    expect(mobileOfferedLayouts(["spreadsheet", "calendar", "list"])).toEqual(["list", "calendar"]);
    expect(mobileOfferedLayouts([])).toEqual([]);
  });

  it("resolves the stored choice when offered, else the first offered", () => {
    expect(resolveActiveLayout("kanban", LAYOUT_ORDER)).toBe("kanban");
    expect(resolveActiveLayout("gantt_chart", ["list", "calendar"])).toBe("list");
    expect(resolveActiveLayout("kanban", ["list"])).toBe("list");
    expect(resolveActiveLayout("kanban", [])).toBe("list");
  });
});
