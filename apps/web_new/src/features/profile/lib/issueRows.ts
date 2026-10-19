// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue-tab row shaping (parity/specs/profile.md §2, PROF-006). The tab
// reads the ownership-selected rows page by page and groups them on the
// client from the stored display choice: priority, state group or
// assignee. State groups come from the projects' state vocabulary, in the
// server's seven-group order, and the list layout can zero-fill the empty
// ones; priority and assignee groups cover only values present in the
// page. An ungrouped tab is one "All work items" section. The local
// renderer stays a seam until the issues area's layouts mount here.

import type { IssueListItem } from "@pidash/api-client";

import { PROFILE_STATE_GROUPS, type ProfileStateGroup } from "./stats.js";

/** Grouping dimensions the profile display menu offers. */
export type ProfileGroupDimension = "priority" | "state" | "assignees";

export const PROFILE_GROUP_LABELS: Record<ProfileGroupDimension, string> = {
  priority: "Priority",
  state: "States",
  assignees: "Assignees",
};

/** Title of the single section an ungrouped tab renders. */
export const UNGROUPED_TITLE = "All work items";

export interface ProfileIssueGroup {
  id: string;
  title: string;
  rows: IssueListItem[];
}

export type PriorityRank = "urgent" | "high" | "medium" | "low" | "none";

export const PRIORITY_ORDER: readonly PriorityRank[] = ["urgent", "high", "medium", "low", "none"];

/** Display label for a priority value. */
export function priorityLabel(priority: string): string {
  return priority.charAt(0).toUpperCase() + priority.slice(1);
}

/** State-group labels, matching the issues area's grouping vocabulary. */
export const STATE_GROUP_TITLES: Record<ProfileStateGroup, string> = {
  backlog: "Backlog",
  unstarted: "Unstarted",
  started: "Started",
  review: "Review",
  test: "Test",
  completed: "Completed",
  cancelled: "Canceled",
};

/**
 * The workspace state vocabulary a page is grouped by: a state id's group
 * and display name. A project's states may be missing while they load, in
 * which case an id falls back to backlog with an empty name.
 */
export interface ProfileStateLookup {
  group: (stateId: string) => ProfileStateGroup | undefined;
  name: (stateId: string) => string | undefined;
}

/** The empty lookup: every id reads as backlog with no name. */
export const EMPTY_STATE_LOOKUP: ProfileStateLookup = {
  group: () => undefined,
  name: () => undefined,
};

/** One project state group reads as one of the profile's seven. */
export function isProfileStateGroup(value: string): value is ProfileStateGroup {
  return (PROFILE_STATE_GROUPS as readonly string[]).includes(value);
}

/** An issue row's state group; absent or unknown states read as backlog. */
export function issueStateGroup(issue: IssueListItem, lookup: ProfileStateLookup): ProfileStateGroup {
  if (issue.state_id === null) return "backlog";
  return lookup.group(issue.state_id) ?? "backlog";
}

/** Member ids an issue carries; empty for unassigned rows. */
export function issueAssigneeIds(issue: IssueListItem): string[] {
  return issue.assignee_ids;
}

function groupTitle(
  dimension: ProfileGroupDimension,
  key: string,
  lookup: ProfileStateLookup,
  memberNames: ReadonlyMap<string, string>
): string {
  switch (dimension) {
    case "priority":
      return priorityLabel(key);
    case "state":
      return STATE_GROUP_TITLES[isProfileStateGroup(key) ? key : "backlog"];
    case "assignees":
      return key === "unassigned" ? "Unassigned" : (memberNames.get(key) ?? key);
  }
}

/**
 * Group the loaded rows. Rows land in the one bucket the dimension
 * resolves for them; unassigned rows group under "Unassigned"; ungrouped
 * pages are one "All work items" section. With `showEmptyGroups`, the
 * state dimension also renders its seven groups with no rows. An empty
 * page answers an empty array (or the seven state groups when asked).
 */
export function groupIssueRows(
  rows: readonly IssueListItem[],
  dimension: ProfileGroupDimension | null,
  lookup: ProfileStateLookup = EMPTY_STATE_LOOKUP,
  options: { showEmptyGroups?: boolean; memberNames?: ReadonlyMap<string, string> } = {}
): ProfileIssueGroup[] {
  const memberNames = options.memberNames ?? new Map<string, string>();
  if (dimension === null) return rows.length === 0 ? [] : [{ id: "all", title: UNGROUPED_TITLE, rows: [...rows] }];
  const groups = new Map<string, IssueListItem[]>();
  if (dimension === "state" && options.showEmptyGroups === true) {
    for (const group of PROFILE_STATE_GROUPS) groups.set(group, []);
  }
  for (const row of rows) {
    const keys =
      dimension === "assignees"
        ? row.assignee_ids.length === 0
          ? ["unassigned"]
          : row.assignee_ids
        : dimension === "priority"
          ? [row.priority]
          : [issueStateGroup(row, lookup)];
    for (const key of keys) {
      const bucket = groups.get(key);
      if (bucket === undefined) groups.set(key, [row]);
      else bucket.push(row);
    }
  }
  return [...groups.entries()]
    .sort(([left], [right]) => compareGroupKeys(dimension, left, right, lookup))
    .map(([key, bucket]) => ({
      id: key,
      title: groupTitle(dimension, key, lookup, memberNames),
      rows: bucket,
    }));
}

function stateRank(key: string, lookup: ProfileStateLookup): number {
  const group = isProfileStateGroup(key) ? key : (lookup.group(key) ?? "backlog");
  const index = PROFILE_STATE_GROUPS.indexOf(group);
  return index === -1 ? PROFILE_STATE_GROUPS.length : index;
}

function compareGroupKeys(
  dimension: ProfileGroupDimension,
  left: string,
  right: string,
  lookup: ProfileStateLookup
): number {
  if (dimension === "priority") {
    return PRIORITY_ORDER.indexOf(left as PriorityRank) - PRIORITY_ORDER.indexOf(right as PriorityRank);
  }
  if (dimension === "state") return stateRank(left, lookup) - stateRank(right, lookup);
  // Assignee groups: "Unassigned" first, the rest by name.
  const unassignedRank = (key: string): number => (key === "unassigned" ? 0 : 1);
  return unassignedRank(left) - unassignedRank(right) || left.localeCompare(right);
}
