// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Calendar drag auto-scroll (parity/specs/issues.md §4, ISS-024). While a
// block drag is in flight, pointer movement inside the scroll container's
// top or bottom edge scrolls the grid so off-screen days stay reachable.

/** Distance from an edge that starts auto-scrolling. */
export const EDGE_SCROLL_THRESHOLD_PX = 40;
/** Largest scroll step for one pointer move. */
export const EDGE_SCROLL_MAX_STEP_PX = 24;

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

/**
 * Pixels to scroll for a pointer at `pointerY`. Negative scrolls up,
 * positive scrolls down, and zero means the pointer is in the safe band.
 */
export function edgeScrollDelta(
  pointerY: number,
  top: number,
  bottom: number,
  threshold: number = EDGE_SCROLL_THRESHOLD_PX,
  maxStep: number = EDGE_SCROLL_MAX_STEP_PX
): number {
  if (bottom <= top) return 0;
  const topZoneEnd = top + threshold;
  if (pointerY < topZoneEnd) {
    const depth = clamp((topZoneEnd - pointerY) / threshold, 0, 1);
    return -Math.max(1, Math.round(depth * maxStep));
  }
  const bottomZoneStart = bottom - threshold;
  if (pointerY > bottomZoneStart) {
    const depth = clamp((pointerY - bottomZoneStart) / threshold, 0, 1);
    return Math.max(1, Math.round(depth * maxStep));
  }
  return 0;
}

/** Scroll a container toward the pointer when it is in an edge zone. */
export function scrollContainerTowardPointer(
  container: Pick<HTMLElement, "getBoundingClientRect" | "scrollBy">,
  pointerY: number
): number {
  const rect = container.getBoundingClientRect();
  const delta = edgeScrollDelta(pointerY, rect.top, rect.bottom);
  if (delta !== 0) container.scrollBy({ top: delta, behavior: "auto" });
  return delta;
}
