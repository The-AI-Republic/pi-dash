// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Options-menu display toggles (parity/specs/pages.md §7.4, §9.1, PAGE-060/
// 072): exactly two rows lead the menu, each flips one stored preference,
// and a checked row wears a tick. Size and face have no row at all.

import { describe, expect, it, vi } from "vitest";

import { PAGE_FULL_WIDTH_LABEL, PAGE_STICKY_TOOLBAR_LABEL, pageDisplayMenuItems } from "./pageDisplayMenu.js";
import { PAGE_PREFERENCES_DEFAULTS } from "./pagePreferences.js";

describe("pageDisplayMenuItems", () => {
  it("offers exactly the two display toggles in the pinned order", () => {
    const items = pageDisplayMenuItems(PAGE_PREFERENCES_DEFAULTS, vi.fn());
    expect(items.map((item) => item.label)).toEqual([PAGE_FULL_WIDTH_LABEL, PAGE_STICKY_TOOLBAR_LABEL]);
    expect(items.some((item) => /font|typeface|typography/i.test(item.label))).toBe(false);
  });

  it("ticks the rows whose preference is on", () => {
    const items = pageDisplayMenuItems(
      { ...PAGE_PREFERENCES_DEFAULTS, full_width: true, sticky_toolbar: false },
      vi.fn()
    );
    expect(items[0]?.icon).toBeTruthy();
    expect(items[1]?.icon).toBeUndefined();
  });

  it("flips full width alone", () => {
    const onChange = vi.fn();
    pageDisplayMenuItems({ ...PAGE_PREFERENCES_DEFAULTS, full_width: false }, onChange)[0]?.onSelect();
    expect(onChange).toHaveBeenCalledWith({ full_width: true });
  });

  it("flips the sticky bar alone", () => {
    const onChange = vi.fn();
    pageDisplayMenuItems({ ...PAGE_PREFERENCES_DEFAULTS, sticky_toolbar: true }, onChange)[1]?.onSelect();
    expect(onChange).toHaveBeenCalledWith({ sticky_toolbar: false });
  });
});
