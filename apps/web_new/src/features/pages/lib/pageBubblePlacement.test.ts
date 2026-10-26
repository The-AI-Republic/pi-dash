// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Selection-bubble placement (PAGE-029, NEWFRONT-927): the bubble flips
// below the caret when its above-caret slot would cover the sticky
// formatting bar, and keeps its default slot everywhere else.

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  PAGE_BUBBLE_ABOVE_LIFT,
  PAGE_BUBBLE_BAR_GAP,
  PAGE_BUBBLE_BELOW_DROP,
  readFormattingBarBottom,
  resolveSelectionBubbleTop,
} from "./pageBubblePlacement.js";

describe("resolveSelectionBubbleTop", () => {
  it("keeps the above-caret slot when no bar is mounted (historical behavior)", () => {
    expect(resolveSelectionBubbleTop(180, null)).toBe(180 - PAGE_BUBBLE_ABOVE_LIFT);
  });

  it("clamps the above-caret slot into the viewport without a bar", () => {
    expect(resolveSelectionBubbleTop(20, null)).toBe(0);
  });

  it("keeps the above-caret slot while it clears the bar", () => {
    expect(resolveSelectionBubbleTop(300, 56)).toBe(300 - PAGE_BUBBLE_ABOVE_LIFT);
  });

  it("keeps the above-caret slot at exactly the clearance boundary", () => {
    const barBottom = 56;
    const anchorTop = barBottom + PAGE_BUBBLE_BAR_GAP + PAGE_BUBBLE_ABOVE_LIFT;
    expect(resolveSelectionBubbleTop(anchorTop, barBottom)).toBe(barBottom + PAGE_BUBBLE_BAR_GAP);
  });

  it("flips below the caret when the above slot would touch the bar (PAGE-029)", () => {
    const barBottom = 56;
    const anchorTop = 70;
    expect(resolveSelectionBubbleTop(anchorTop, barBottom)).toBe(anchorTop + PAGE_BUBBLE_BELOW_DROP);
  });

  it("clamps the flipped bubble out from under the bar for hidden carets", () => {
    // A caret scrolled behind the sticky bar: below-caret would still
    // overlap, so the bubble parks just under the bar's bottom edge.
    expect(resolveSelectionBubbleTop(10, 56)).toBe(56 + PAGE_BUBBLE_BAR_GAP);
  });
});

describe("readFormattingBarBottom", () => {
  afterEach(() => {
    document.body.innerHTML = "";
    vi.restoreAllMocks();
  });

  it("answers null when the bar is not mounted", () => {
    expect(readFormattingBarBottom("page-toolbar-container")).toBeNull();
  });

  it("reads the mounted bar's bottom edge", () => {
    const bar = document.createElement("div");
    bar.id = "page-toolbar-container";
    document.body.appendChild(bar);
    vi.spyOn(bar, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 800, 56));
    expect(readFormattingBarBottom("page-toolbar-container")).toBe(56);
  });
});
