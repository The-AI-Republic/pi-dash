// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board drag auto-scroll (parity/specs/issues.md §5, ISS-043). While a
// card drag is in flight, a pointer held near the board's edges scrolls
// the board horizontally and vertically, and a pointer held near the
// hovered column's edges scrolls that column, so off-screen drop targets
// stay reachable. The edge math is the calendar's; only the axes differ.

import { edgeScrollDelta } from "../calendar/autoScroll.js";

/** Interval between scroll steps while a drag holds still. */
export const BOARD_AUTOSCROLL_INTERVAL_MS = 50;

export interface ScrollableNode {
  getBoundingClientRect: () => { left: number; top: number; right: number; bottom: number };
  scrollBy: (options: { left?: number; top?: number; behavior?: ScrollBehavior }) => void;
}

/**
 * Scroll steps for a pointer at `(x, y)`: horizontal toward the left and
 * right edges, vertical toward the top and bottom. Axes the caller does
 * not ask for stay zero.
 */
export function boardScrollStep(
  node: Pick<ScrollableNode, "getBoundingClientRect">,
  x: number,
  y: number,
  axes: { horizontal: boolean; vertical: boolean }
): { x: number; y: number } {
  const rect = node.getBoundingClientRect();
  return {
    x: axes.horizontal ? edgeScrollDelta(x, rect.left, rect.right) : 0,
    y: axes.vertical ? edgeScrollDelta(y, rect.top, rect.bottom) : 0,
  };
}

/** Scroll the node toward the pointer when it rests in an edge zone. */
export function scrollNodeTowardPoint(
  node: ScrollableNode,
  x: number,
  y: number,
  axes: { horizontal: boolean; vertical: boolean }
): { x: number; y: number } {
  const step = boardScrollStep(node, x, y, axes);
  if (step.x !== 0 || step.y !== 0) node.scrollBy({ left: step.x, top: step.y, behavior: "auto" });
  return step;
}
