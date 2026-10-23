// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Activity feed rules (parity/specs/issues.md §16, ISS-194–196, plus the
// cloud worklog category in ISS-206). Pure helpers behind the merged
// timeline: which category an activity belongs to, how activities and
// comments interleave by creation time, how the toggles narrow the
// result, what each stored field reads as, and how the sort and filter
// choices persist in the browser. The component layer only renders what
// these return.

import type { IssueActivity, IssueComment } from "@pidash/api-client";

import { formatMinutes } from "../../../shared/lib/formatDuration.js";

/** The four toggle categories; only creation rows bypass them. */
export const FEED_FILTERS = ["ACTIVITY", "COMMENT", "STATE", "ASSIGNEE"] as const;
/** Cloud-only fifth toggle (ISS-206): logged-time entries. Absent in OSS. */
export const WORKLOG_FILTER = "WORKLOG" as const;
export type FeedFilter = (typeof FEED_FILTERS)[number] | typeof WORKLOG_FILTER;
/** All five toggles in menu order, for cloud builds. */
export const CLOUD_FEED_FILTERS: readonly FeedFilter[] = [...FEED_FILTERS, WORKLOG_FILTER];

/** Toggle labels in menu order. */
export const FEED_FILTER_LABELS: Record<FeedFilter, string> = {
  ACTIVITY: "Updates",
  COMMENT: "Comments",
  STATE: "State",
  ASSIGNEE: "Assignee",
  WORKLOG: "Worklog",
};

export type FeedSortOrder = "asc" | "desc";

export const DEFAULT_SORT: FeedSortOrder = "asc";
export const DEFAULT_FILTERS: readonly FeedFilter[] = FEED_FILTERS;

/** Browser-local keys the choices live under (ISS-195/196). */
export const FILTERS_STORAGE_KEY = "issue_activity_filters";
export const SORT_STORAGE_KEY = "activity_sort_order";

/** Rows that only ever move, rename or add files are "updates". */
export function activityFilterOf(activity: Pick<IssueActivity, "field">): FeedFilter | "DEFAULT" {
  if (activity.field === null) return "DEFAULT";
  if (activity.field === "state") return "STATE";
  if (activity.field === "assignees") return "ASSIGNEE";
  if (activity.field === "worklog") return WORKLOG_FILTER;
  return "ACTIVITY";
}

export type FeedEntry =
  | { readonly kind: "activity"; readonly id: string; readonly createdAt: string; readonly activity: IssueActivity }
  | { readonly kind: "comment"; readonly id: string; readonly createdAt: string; readonly comment: IssueComment };

/** One timeline row per source row, keyed by the source id. */
export function buildFeed(activities: readonly IssueActivity[], comments: readonly IssueComment[]): FeedEntry[] {
  const entries: FeedEntry[] = [];
  for (const activity of activities) {
    entries.push({ kind: "activity", id: activity.id, createdAt: activity.created_at, activity });
  }
  for (const comment of comments) {
    entries.push({ kind: "comment", id: comment.id, createdAt: comment.created_at, comment });
  }
  return entries;
}

function createdAtMs(entry: FeedEntry): number {
  const ms = Date.parse(entry.createdAt);
  return Number.isNaN(ms) ? 0 : ms;
}

/** Oldest first, or newest first; equal timestamps keep source order. */
export function sortFeed(entries: readonly FeedEntry[], order: FeedSortOrder): FeedEntry[] {
  const ranked = entries.map((entry, index) => ({ entry, index, at: createdAtMs(entry) }));
  ranked.sort((left, right) => {
    if (left.at !== right.at) return order === "asc" ? left.at - right.at : right.at - left.at;
    return left.index - right.index;
  });
  return ranked.map((row) => row.entry);
}

/** Whether a toggle set keeps one entry on screen. */
export function feedEntryVisible(entry: FeedEntry, filters: readonly FeedFilter[]): boolean {
  if (entry.kind === "comment") return filters.includes("COMMENT");
  const category = activityFilterOf(entry.activity);
  return category === "DEFAULT" || filters.includes(category);
}

export function filterFeed(entries: readonly FeedEntry[], filters: readonly FeedFilter[]): FeedEntry[] {
  return entries.filter((entry) => feedEntryVisible(entry, filters));
}

/**
 * Merge, filter and sort in one pass. The component calls this with the
 * two source lists and the two stored choices.
 */
export function composeFeed(
  activities: readonly IssueActivity[],
  comments: readonly IssueComment[],
  filters: readonly FeedFilter[],
  order: FeedSortOrder
): FeedEntry[] {
  return sortFeed(filterFeed(buildFeed(activities, comments), filters), order);
}

/**
 * The filter button marks itself while any toggle is off (ISS-195). The
 * available set defaults to the four OSS toggles; cloud builds pass all
 * five so four-of-five still reads as narrowed.
 */
export function isFilterNarrowed(
  filters: readonly FeedFilter[],
  available: readonly FeedFilter[] = FEED_FILTERS
): boolean {
  return filters.length < available.length;
}

/**
 * Toggle one filter. Turning the last one off is a no-op, and the
 * resulting list keeps menu order rather than click order so the stored
 * value is stable.
 */
export function toggleFeedFilter(
  current: readonly FeedFilter[],
  filter: FeedFilter,
  available: readonly FeedFilter[] = FEED_FILTERS
): FeedFilter[] {
  if (current.includes(filter)) {
    if (current.length === 1) return [...current];
    return available.filter((candidate) => candidate !== filter && current.includes(candidate));
  }
  return available.filter((candidate) => candidate === filter || current.includes(candidate));
}

/** The other order when the sort control is pressed. */
export function flipSort(order: FeedSortOrder): FeedSortOrder {
  return order === "asc" ? "desc" : "asc";
}

/**
 * Read the stored toggles, falling back to every available one for junk
 * or empty. Stored values outside the available set drop away, so a
 * cloud-stored worklog toggle never leaks into the OSS menu.
 */
export function parseStoredFilters(raw: string | null, available: readonly FeedFilter[] = FEED_FILTERS): FeedFilter[] {
  if (raw === null) return [...available];
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return [...available];
  }
  if (!Array.isArray(value)) return [...available];
  const kept = available.filter((filter) => value.includes(filter));
  return kept.length > 0 ? kept : [...available];
}

/** Read the stored order; anything but "desc" reads as ascending. */
export function parseStoredSort(raw: string | null): FeedSortOrder {
  if (raw === null) return DEFAULT_SORT;
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return DEFAULT_SORT;
  }
  return value === "desc" ? "desc" : DEFAULT_SORT;
}

/**
 * What one property activity reads as. Unknown fields produce no row at
 * all (the old feed renders nothing for them), so this returns null for
 * fields the timeline does not describe.
 */
export function activitySentence(
  activity: Pick<IssueActivity, "field" | "verb" | "old_value" | "new_value">
): string | null {
  const created = activity.verb === "created";
  const value = activity.new_value ?? "";
  const previous = activity.old_value ?? "";
  switch (activity.field) {
    case null:
      return activity.verb === "deleted" ? "deleted the work item." : "created the work item.";
    // The server stamps a deletion with field "issue" (parity/specs/comments.md §4.1).
    case "issue":
      return "deleted the work item.";
    case "name":
      return `renamed the work item to “${value}”.`;
    case "description":
      return "edited the description.";
    case "state":
      return `moved the state to ${value}.`;
    case "assignees":
      return previous === "" ? `assigned the work item to ${value}.` : `removed ${previous} from the assignees.`;
    case "priority":
      return `set the priority to ${value}.`;
    case "estimate_point":
    case "estimate_points":
    case "estimate_categories":
      return value === "" ? "cleared the estimate." : `set the estimate to ${value}.`;
    case "parent":
      return value === "" ? `removed the parent ${previous}.` : `set the parent to ${value}.`;
    case "blocking":
    case "blocked_by":
    case "duplicate":
    case "relates_to":
      return relationSentence(activity.field as RelationField, previous, value);
    case "start_date":
      return value === "" ? "cleared the start date." : `set the start date to ${value}.`;
    case "target_date":
      return value === "" ? "cleared the due date." : `set the due date to ${value}.`;
    case "cycles":
      if (created) return `added the work item to the cycle ${value}.`;
      return activity.verb === "deleted" || value === ""
        ? `removed the work item from the cycle ${previous}.`
        : `moved the work item to the cycle ${value}.`;
    case "modules":
      if (created) return `added the work item to the module ${value}.`;
      return activity.verb === "deleted" || value === ""
        ? `removed the work item from the module ${previous}.`
        : `moved the work item to the module ${value}.`;
    case "labels":
      return previous === "" ? `added the label ${value}.` : `removed the label ${previous}.`;
    case "link":
      if (created) return "attached a link.";
      return activity.verb === "deleted" ? "removed the attached link." : "changed the attached link.";
    case "attachment":
      return created ? "attached a file." : "removed an attachment.";
    case "archived_at":
      return value === "restore" ? "restored the work item." : "archived the work item.";
    case "agent_wait":
      return `started waiting on ${value} (${previous}).`;
    case "agent_wake":
      return `resumed after ${value} was ${previous}.`;
    case "intake":
    case "inbox":
      return inboxSentence(activity.verb);
    case "worklog":
      return worklogSentence(activity.verb, previous, value);
    default:
      return null;
  }
}

/** One of the four relation fields, each with its add and drop verbs. */
type RelationField = "blocking" | "blocked_by" | "duplicate" | "relates_to";

function relationSentence(field: RelationField, previous: string, value: string): string {
  const adding = previous === "";
  const target = adding ? value : previous;
  const verb = {
    blocking: adding ? "marked the work item as blocking" : "dropped the blocking link to",
    blocked_by: adding ? "marked the work item as blocked by" : "dropped the blocked-by link to",
    duplicate: adding ? "marked the work item as a duplicate of" : "dropped the duplicate link to",
    relates_to: adding ? "linked the work item to" : "unlinked the work item from",
  }[field];
  return `${verb} ${target}.`;
}

function inboxSentence(verb: string): string {
  switch (verb) {
    case "1":
      return "accepted the intake item.";
    case "-1":
      return "declined the intake item.";
    case "0":
      return "snoozed the intake item.";
    case "2":
      return "declined the intake item as a duplicate.";
    default:
      return "changed the intake status.";
  }
}

/**
 * A worklog activity as a sentence (ISS-206, cloud leg). The server
 * carries minutes in the old/new values; unparseable values fall back
 * to a plain verb sentence instead of rendering nothing.
 */
function worklogSentence(verb: string, previous: string, value: string): string {
  if (verb === "deleted") return "removed logged time.";
  if (verb === "updated") {
    const before = Number(previous);
    const after = Number(value);
    const bothValid = Number.isInteger(before) && before > 0 && Number.isInteger(after) && after > 0;
    if (bothValid && before !== after) {
      return `changed logged time from ${formatMinutes(before)} to ${formatMinutes(after)}.`;
    }
    return "edited a time entry.";
  }
  if (value === "") return "logged time.";
  const minutes = Number(value);
  return Number.isInteger(minutes) && minutes > 0 ? `logged ${formatMinutes(minutes)}.` : "logged time.";
}

/** Who the row credits: the actor, or the product for its own actions. */
export function activityActorName(activity: Pick<IssueActivity, "field" | "new_value" | "actor_detail">): string {
  if (activity.field === "agent_wait" || activity.field === "agent_wake") return "Pi Dash";
  if (activity.field === "archived_at" && activity.new_value === "archive") return "Pi Dash";
  return activity.actor_detail?.display_name ?? "Unknown author";
}

/**
 * Stable storage value for the toggle set. The available set defaults to
 * the four OSS toggles; cloud builds pass all five so a stored worklog
 * toggle survives the round trip instead of being dropped on write.
 */
export function serializeFilters(
  filters: readonly FeedFilter[],
  available: readonly FeedFilter[] = FEED_FILTERS
): string {
  return JSON.stringify(available.filter((filter) => filters.includes(filter)));
}

export function serializeSort(order: FeedSortOrder): string {
  return JSON.stringify(order);
}
