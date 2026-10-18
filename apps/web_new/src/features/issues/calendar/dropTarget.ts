// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Resolve the calendar day under a pointer (parity/specs/issues.md §4,
// ISS-024). Pointer capture keeps enter/up events on the drag source, so
// the drop target is resolved from the document at release time instead
// of relying on the tile's own pointer handlers.

export const CALENDAR_DAY_SELECTOR = "[data-calendar-day]";

interface PointDocument {
  elementFromPoint?: ((x: number, y: number) => Element | null) | undefined;
  querySelectorAll: (selectors: string) => ArrayLike<Element>;
}

interface PointRect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

function isPointInside(rect: PointRect, x: number, y: number): boolean {
  if (rect.right <= rect.left || rect.bottom <= rect.top) return false;
  return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
}

/** ISO day of the calendar tile under `(x, y)`, or null. */
export function calendarDayAtPoint(doc: PointDocument | null | undefined, x: number, y: number): string | null {
  if (doc === null || doc === undefined) return null;
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
  if (typeof doc.elementFromPoint === "function") {
    const hit = doc.elementFromPoint(x, y);
    const tile = hit?.closest?.(CALENDAR_DAY_SELECTOR) ?? null;
    const day = tile?.getAttribute("data-calendar-day") ?? null;
    if (day !== null && day !== "") return day;
  }
  for (const tile of Array.from(doc.querySelectorAll(CALENDAR_DAY_SELECTOR))) {
    const day = tile.getAttribute("data-calendar-day");
    if (day === null || day === "") continue;
    if (isPointInside(tile.getBoundingClientRect(), x, y)) return day;
  }
  return null;
}
