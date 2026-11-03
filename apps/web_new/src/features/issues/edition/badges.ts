// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud-only detail accents (parity/specs/issues.md §20/§23, ISS-236):
// the overdue badge beside the due date and the transfer-hop note under
// the cycle row. Both render only where the edition flag is on; the
// community build shows the plain date and cycle, never the words the
// oracle forbids.

import { edition } from "../../../core/edition/index.js";
import { DETAIL_BADGES_FLAG, isEditionFlagOn } from "./flags.js";

export { DETAIL_BADGES_FLAG };

/** True when this build draws the overdue and transfer accents. */
export function showsDetailAccents(flags: Record<string, boolean> = edition.flags): boolean {
  return isEditionFlagOn(DETAIL_BADGES_FLAG, flags);
}

/** Whole days between the due day and today; negative when past due. */
export function overdueDays(dueDay: string, today: string): number {
  const due = Date.parse(`${dueDay.slice(0, 10)}T00:00:00Z`);
  const now = Date.parse(`${today.slice(0, 10)}T00:00:00Z`);
  if (Number.isNaN(due) || Number.isNaN(now)) return 0;
  return Math.round((due - now) / 86_400_000);
}

/**
 * The badge's reading for an overdue day: "Due today", "1 day overdue",
 * "N days overdue". A future day reads as the empty string, so a caller
 * that forgot its predicate renders nothing visible.
 */
export function overdueBadgeLabel(dueDay: string, today: string): string {
  const days = overdueDays(dueDay, today);
  if (days > 0) return "";
  if (days === 0) return "Due today";
  const late = -days;
  return late === 1 ? "1 day overdue" : `${late} days overdue`;
}

/** Notice body once a cycle move confirms. */
export function transferHopNotice(cycleName: string): string {
  return `Moved to ${cycleName}.`;
}
