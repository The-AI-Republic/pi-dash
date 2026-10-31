// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board drag auto-scroll (parity/specs/issues.md §5, ISS-043): edge
// steps on the requested axes, and no scroll from the safe band.
import { describe, expect, it, type Mock, vi } from "vitest";

import {
  BOARD_AUTOSCROLL_INTERVAL_MS,
  boardScrollStep,
  scrollNodeTowardPoint,
  type ScrollableNode,
} from "./autoScroll.js";

function node(): ScrollableNode & {
  scrollBy: Mock<(options: { left?: number; top?: number; behavior?: ScrollBehavior }) => void>;
} {
  return {
    getBoundingClientRect: () => ({ left: 0, top: 0, right: 800, bottom: 600 }),
    scrollBy: vi.fn(),
  };
}

describe("boardScrollStep", () => {
  it("steps horizontally near the left and right edges", () => {
    expect(boardScrollStep(node(), 5, 300, { horizontal: true, vertical: false }).x).toBeLessThan(0);
    expect(boardScrollStep(node(), 795, 300, { horizontal: true, vertical: false }).x).toBeGreaterThan(0);
    expect(boardScrollStep(node(), 5, 300, { horizontal: true, vertical: false }).y).toBe(0);
  });

  it("steps vertically near the top and bottom edges", () => {
    expect(boardScrollStep(node(), 400, 5, { horizontal: false, vertical: true }).y).toBeLessThan(0);
    expect(boardScrollStep(node(), 400, 595, { horizontal: false, vertical: true }).y).toBeGreaterThan(0);
    expect(boardScrollStep(node(), 400, 5, { horizontal: false, vertical: true }).x).toBe(0);
  });

  it("stays put in the safe band", () => {
    expect(boardScrollStep(node(), 400, 300, { horizontal: true, vertical: true })).toEqual({ x: 0, y: 0 });
  });

  it("paces held drags at the auto-scroll interval", () => {
    expect(BOARD_AUTOSCROLL_INTERVAL_MS).toBe(50);
  });
});

describe("scrollNodeTowardPoint", () => {
  it("scrolls toward the pointer in an edge zone", () => {
    const target = node();
    const step = scrollNodeTowardPoint(target, 795, 300, { horizontal: true, vertical: true });
    expect(step.x).toBeGreaterThan(0);
    expect(target.scrollBy).toHaveBeenCalledTimes(1);
    expect(target.scrollBy).toHaveBeenCalledWith({ left: step.x, top: 0, behavior: "auto" });
  });

  it("leaves a centered pointer alone", () => {
    const target = node();
    expect(scrollNodeTowardPoint(target, 400, 300, { horizontal: true, vertical: true })).toEqual({
      x: 0,
      y: 0,
    });
    expect(target.scrollBy).not.toHaveBeenCalled();
  });
});
