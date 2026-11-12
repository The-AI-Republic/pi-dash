// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Body popover placement (parity/specs/pages.md §6.1–§6.4, PAGE-037,
// PAGE-041–043). The slash palette, the emoji picker, the entity picker
// and the mention popup all open beside the caret in viewport
// coordinates, so one rule keeps every one of them on screen: open below
// the caret, flip above when the lower edge is short, and clamp the box
// inside the viewport margin. Nothing here touches the DOM, so the rule
// is unit-testable on its own.

/** Space kept between a popover and the viewport's own edges. */
export const PAGE_POPOVER_VIEWPORT_MARGIN = 8;

/** Space kept between the caret and the popover that follows it. */
export const PAGE_POPOVER_ANCHOR_GAP = 24;

/** A width and height pair: a viewport, or a popover's painted box. */
export interface PopoverSize {
  width: number;
  height: number;
}

/** Where a popover wants to sit, in viewport pixels. */
export interface PopoverPlacement {
  top: number;
  left: number;
}

/**
 * True when the body may open a trigger popover. A dismissal records the
 * document it covered; the trigger stays shut while that document is
 * unchanged, so the selection pass that follows Escape cannot reopen the
 * palette (or the mention popup) it just closed. Any edit clears it.
 */
export function pageTriggerReopened(dismissedStamp: string | null, html: string): boolean {
  return dismissedStamp === null || dismissedStamp !== html;
}

/** The largest box the viewport can show without crossing its margin. */
export function pagePopoverMaxSize(viewport: PopoverSize): PopoverSize {
  return {
    width: Math.max(0, viewport.width - 2 * PAGE_POPOVER_VIEWPORT_MARGIN),
    height: Math.max(0, viewport.height - 2 * PAGE_POPOVER_VIEWPORT_MARGIN),
  };
}

/**
 * Place one popover beside the caret. An unmeasured viewport (before the
 * first layout pass) places it straight below the caret rather than
 * clamping against zeroes. A box that does not fit below flips above the
 * caret; whatever is still out of bounds is clamped to the margin, so the
 * caller's own max height/scroll takes over from there.
 */
export function pagePopoverPlacement(
  anchor: { top: number; left: number },
  size: PopoverSize,
  viewport: PopoverSize
): PopoverPlacement {
  const below = anchor.top + PAGE_POPOVER_ANCHOR_GAP;
  if (viewport.width <= 0 || viewport.height <= 0) {
    return { top: Math.max(0, Math.round(below)), left: Math.max(0, Math.round(anchor.left)) };
  }
  const lowest = viewport.height - PAGE_POPOVER_VIEWPORT_MARGIN - size.height;
  const above = anchor.top - PAGE_POPOVER_ANCHOR_GAP - size.height;
  const preferred = below + size.height > viewport.height - PAGE_POPOVER_VIEWPORT_MARGIN ? above : below;
  const rightmost = viewport.width - PAGE_POPOVER_VIEWPORT_MARGIN - size.width;
  return {
    top: Math.round(clamp(preferred, PAGE_POPOVER_VIEWPORT_MARGIN, lowest)),
    left: Math.round(clamp(anchor.left, PAGE_POPOVER_VIEWPORT_MARGIN, rightmost)),
  };
}

/** One value held inside a range whose ends may cross on a tiny viewport. */
function clamp(value: number, low: number, high: number): number {
  if (high < low) return low;
  return Math.min(Math.max(value, low), high);
}
