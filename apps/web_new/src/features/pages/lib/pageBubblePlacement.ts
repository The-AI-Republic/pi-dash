// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Selection-bubble placement (PAGE-029, NEWFRONT-927). The floating
// formatting/link bubble sits above the caret by default, but a
// first-line selection would land it on top of the sticky formatting
// bar — where its higher stacking order swallows the bar's own clicks.
// The policy keeps the above-caret default only while it clears the
// bar's measured bottom edge, and flips below the caret otherwise, so
// the bubble never covers the bar's controls. The bubble itself stays
// fully visible either way: its formatting buttons (PAGE-037) and its
// in-place link editor (PAGE-043) are both reachable in both slots.

/** Lift of the bubble's top edge above the caret anchor (viewport px). */
export const PAGE_BUBBLE_ABOVE_LIFT = 48;
/** Drop of the flipped bubble's top edge below the caret anchor (viewport px). */
export const PAGE_BUBBLE_BELOW_DROP = 24;
/** Clearance kept between the bubble and the sticky bar (viewport px). */
export const PAGE_BUBBLE_BAR_GAP = 8;

/**
 * The bubble wrapper's top edge in viewport px. Without a formatting
 * bar the bubble keeps its historical above-caret slot, clamped into
 * the viewport. With a bar, the above slot stands only while it clears
 * the bar's bottom edge; otherwise the bubble flips below the caret,
 * clamped out from under the bar for carets scrolled behind it.
 */
export function resolveSelectionBubbleTop(anchorTop: number, barBottom: number | null): number {
  const above = anchorTop - PAGE_BUBBLE_ABOVE_LIFT;
  if (barBottom === null) return Math.max(0, above);
  if (above >= barBottom + PAGE_BUBBLE_BAR_GAP) return above;
  return Math.max(barBottom + PAGE_BUBBLE_BAR_GAP, anchorTop + PAGE_BUBBLE_BELOW_DROP);
}

/**
 * The sticky bar's bottom edge in viewport px, or null when the bar is
 * not mounted (read-only pages and the sticky-pref-off state render no
 * bar, so the bubble keeps its default slot there).
 */
export function readFormattingBarBottom(barId: string): number | null {
  const bar = document.getElementById(barId);
  if (bar === null) return null;
  return bar.getBoundingClientRect().bottom;
}
