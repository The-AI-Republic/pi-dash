// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Preview-card date rules (parity/specs/issues.md §11, ISS-133). The card
// shows one line: the range when both ends are set, the start when only
// that is set, the due date when only that is set, and nothing when the
// record carries no dates at all. A due date highlights once it has
// passed, but only while the record is unfinished — completed and
// cancelled rows keep the plain tone.

/** Closed vocabulary of state groups the overdue rule reads. */
const FINISHED_GROUPS = new Set(["completed", "cancelled"]);

/** What the card's date line says, or null when it renders none. */
export interface PreviewDateLine {
  kind: "range" | "start" | "due";
  text: string;
  overdue: boolean;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/**
 * "MMM dd, yyyy" without timezone math: the values are calendar days, so
 * they must render exactly as stored wherever the viewer sits.
 */
export function formatCalendarDay(iso: string): string {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso);
  if (match === null) return iso;
  const month = MONTHS[Number(match[2]) - 1] ?? "";
  return `${month} ${match[3]}, ${match[1]}`;
}

/** Whether `dueIso` lies on or before `todayIso` (day strings compare lexically). */
export function isOverdue(dueIso: string | null, todayIso: string): boolean {
  return dueIso !== null && dueIso <= todayIso;
}

/** The card's single date line from the record's two ends. */
export function previewDateLine(
  startIso: string | null,
  dueIso: string | null,
  stateGroup: string | null,
  todayIso: string
): PreviewDateLine | null {
  const finished = stateGroup !== null && FINISHED_GROUPS.has(stateGroup);
  const overdueDue = !finished && isOverdue(dueIso, todayIso);
  if (startIso !== null && dueIso !== null) {
    return {
      kind: "range",
      text: `${formatCalendarDay(startIso)} - ${formatCalendarDay(dueIso)}`,
      overdue: overdueDue,
    };
  }
  if (startIso !== null && dueIso === null) {
    return { kind: "start", text: formatCalendarDay(startIso), overdue: false };
  }
  if (startIso === null && dueIso !== null) {
    return { kind: "due", text: formatCalendarDay(dueIso), overdue: overdueDue };
  }
  return null;
}
