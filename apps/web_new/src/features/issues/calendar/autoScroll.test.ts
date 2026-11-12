// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import { edgeScrollDelta, scrollContainerTowardPointer } from "./autoScroll.js";

describe("calendar auto-scroll", () => {
  it("scrolls up near the top edge and down near the bottom edge", () => {
    expect(edgeScrollDelta(101, 100, 500)).toBe(-23);
    expect(edgeScrollDelta(499, 100, 500)).toBe(23);
    expect(edgeScrollDelta(300, 100, 500)).toBe(0);
  });

  it("scales the step with edge depth and clamps beyond the edge", () => {
    expect(edgeScrollDelta(120, 100, 500, 40, 20)).toBe(-10);
    expect(edgeScrollDelta(480, 100, 500, 40, 20)).toBe(10);
    expect(edgeScrollDelta(0, 100, 500, 40, 20)).toBe(-20);
    expect(edgeScrollDelta(900, 100, 500, 40, 20)).toBe(20);
  });

  it("ignores collapsed or invalid containers", () => {
    expect(edgeScrollDelta(100, 200, 200)).toBe(0);
    expect(edgeScrollDelta(100, 300, 200)).toBe(0);
  });

  it("calls scrollBy only when the pointer is in an edge zone", () => {
    const container = {
      getBoundingClientRect: () => ({ top: 100, bottom: 500 }) as DOMRect,
      scrollBy: vi.fn(),
    };
    expect(scrollContainerTowardPointer(container, 499)).toBe(23);
    expect(container.scrollBy).toHaveBeenCalledWith({ top: 23, behavior: "auto" });
    expect(scrollContainerTowardPointer(container, 300)).toBe(0);
    expect(container.scrollBy).toHaveBeenCalledTimes(1);
  });
});
