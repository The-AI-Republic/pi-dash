// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public issue overlay rules (parity/specs/space.md §3.7, SPC-030/040–046).
// Pure helpers only: which address value names an issue, the date the
// property panel prints, when that date reads as overdue, and the two
// addresses the overlay writes (open with peekId, close without it while
// keeping every other parameter).

import { normalizeBoardHref, type SpaceBoardSearch } from "./address.js";

/** The public issue id shape; anything else cannot be fetched. */
const ISSUE_ID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** States whose due date never reads as overdue, however late it is. */
const CLOSED_STATE_GROUPS = new Set(["completed", "cancelled"]);

/** Whether the address value can name a public issue at all. */
export function isSpaceIssueId(value: string | undefined): value is string {
  return value !== undefined && ISSUE_ID_PATTERN.test(value);
}

/** Property-panel date shape; the same short form the cards use. */
const PROPERTY_DATE_FORMAT = new Intl.DateTimeFormat("en-US", {
  month: "short",
  day: "numeric",
  year: "numeric",
  timeZone: "UTC",
});

/** The due date the property panel prints, or null when unset/unreadable. */
export function formatSpacePropertyDate(isoDate: string | null): string | null {
  if (isoDate === null) return null;
  const parsed = new Date(`${isoDate}T00:00:00Z`);
  if (Number.isNaN(parsed.getTime())) return null;
  return PROPERTY_DATE_FORMAT.format(parsed);
}

/**
 * Whether a due date is overdue: on or before today, on an issue whose
 * state group is still open (SPC-044). A missing date or state group is
 * never overdue.
 */
export function isSpaceDueOverdue(isoDate: string | null, stateGroup: string | null, now: Date = new Date()): boolean {
  if (isoDate === null || stateGroup === null) return false;
  if (CLOSED_STATE_GROUPS.has(stateGroup)) return false;
  const parsed = new Date(`${isoDate}T00:00:00Z`);
  if (Number.isNaN(parsed.getTime())) return false;
  const today = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate());
  return parsed.getTime() <= today;
}

/** The address that opens one issue over the current board position. */
export function spacePeekOpenHref(anchor: string, search: SpaceBoardSearch, issueId: string): string {
  return normalizeBoardHref(anchor, { ...search, peekId: issueId });
}

/**
 * The address left after closing the overlay: only `peekId` disappears,
 * so the layout and every filter key survive (SPC-042).
 */
export function spacePeekCloseHref(anchor: string, search: SpaceBoardSearch): string {
  const { peekId: _dropped, ...rest } = search;
  return normalizeBoardHref(anchor, rest);
}
