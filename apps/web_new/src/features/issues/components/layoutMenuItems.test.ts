// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { isMenuCustom, isMenuSeparator } from "@pidash/kit";
import { describe, expect, it, vi } from "vitest";
import { LAYOUT_LABELS, LAYOUT_ORDER } from "../layouts/kinds.js";
import { layoutMenuItems } from "./layoutMenuItems.js";

describe("layoutMenuItems", () => {
  it("offers icon-plus-label rows in the given order", () => {
    const items = layoutMenuItems(LAYOUT_ORDER, "list", () => {});
    expect(items).toHaveLength(5);
    for (const [index, item] of items.entries()) {
      if (isMenuSeparator(item) || isMenuCustom(item)) throw new Error("expected a plain action row");
      expect(item.label).toBe(LAYOUT_LABELS[LAYOUT_ORDER[index]!]);
      expect(item.itemKey).toBe(LAYOUT_ORDER[index]);
      expect(item.icon).toBeDefined();
    }
  });

  it("marks only the active row with rich content", () => {
    const items = layoutMenuItems(LAYOUT_ORDER, "calendar", () => {});
    for (const item of items) {
      if (isMenuSeparator(item) || isMenuCustom(item)) throw new Error("expected a plain action row");
      expect(item.content !== undefined).toBe(item.label === "Calendar");
    }
  });

  it("switches on inactive rows and no-ops on the active one", () => {
    const onSwitch = vi.fn();
    const items = layoutMenuItems(LAYOUT_ORDER, "list", onSwitch);
    for (const item of items) {
      if (isMenuSeparator(item) || isMenuCustom(item)) throw new Error("expected a plain action row");
      item.onSelect();
    }
    expect(onSwitch).toHaveBeenCalledTimes(4);
    expect(onSwitch).not.toHaveBeenCalledWith("list");
    expect(onSwitch).toHaveBeenCalledWith("gantt_chart");
  });
});
