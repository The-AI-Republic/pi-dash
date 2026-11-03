// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Worklog presentation rules (parity/specs/issues.md §16, ISS-206, cloud
// leg). Totals, row bylines, and the month-day dates the sidebar rows
// render, plus the shared titles and notices. The month names are pinned
// here so entry dates read identically in every browser locale.

import type { WorklogEntry } from "@pidash/api-client";

/** Sidebar section heading, shared by the section and its empty check. */
export const WORKLOG_SECTION_TITLE = "Worklog";
/** Header control that opens the log-time dialog. */
export const WORKLOG_LOG_ACTION = "Log time";
/** Dialog heading for a fresh entry. */
export const WORKLOG_DIALOG_TITLE_LOG = "Log time";
/** Dialog heading when editing an existing entry. */
export const WORKLOG_DIALOG_TITLE_EDIT = "Edit time entry";
/** Empty-list line while entries exist nowhere yet. */
export const WORKLOG_EMPTY = "No time logged yet.";
/** Section error line when the entry list fails to load. */
export const WORKLOG_LOAD_FAILED = "Couldn't load the worklog.";
/** Success notice after an entry is logged. */
export const WORKLOG_LOGGED_TITLE = "Time logged";
/** Failure notice when the server refuses a new entry. */
export const WORKLOG_LOG_FAILED_TITLE = "Couldn't log time";
/** Success notice after an entry is edited. */
export const WORKLOG_UPDATED_TITLE = "Time entry updated";
/** Failure notice when the server refuses an edit. */
export const WORKLOG_UPDATE_FAILED_TITLE = "Couldn't update the time entry";
/** Success notice after an entry is removed. */
export const WORKLOG_DELETED_TITLE = "Time entry removed";
/** Failure notice when the server refuses a removal. */
export const WORKLOG_DELETE_FAILED_TITLE = "Couldn't remove the time entry";

/** Whole minutes logged across a set of entries. */
export function totalWorklogMinutes(entries: readonly Pick<WorklogEntry, "minutes">[]): number {
  return entries.reduce((sum, entry) => sum + entry.minutes, 0);
}

const MONTH_SHORT = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/**
 * A logged-on day as "Oct 20, 2026". Malformed input passes through
 * untouched so a bad server value stays visible instead of crashing.
 */
export function formatLoggedOn(loggedOn: string): string {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(loggedOn);
  if (match === null) return loggedOn;
  const month = MONTH_SHORT[Number(match[2]) - 1];
  if (month === undefined) return loggedOn;
  return `${month} ${Number(match[3])}, ${match[1]}`;
}

/**
 * Attribution prefix for someone else's entry ("Ada L · "), or null for
 * the viewer's own entries and for entries with no known actor, which
 * read unattributed. Unnamed actors read as "Someone".
 */
export function worklogActorLabel(
  entry: Pick<WorklogEntry, "actor" | "actor_detail">,
  viewerId: string | null
): string | null {
  if (viewerId !== null && entry.actor === viewerId) return null;
  const name = entry.actor_detail?.display_name.trim() ?? "";
  return `${name === "" ? "Someone" : name} · `;
}
