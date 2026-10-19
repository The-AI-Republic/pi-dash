// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue-tab row shaping (parity/specs/profile.md §2, PROF-006). The tab
// reads one ownership-selected page and renders it in the stored layout.
// Grouping is client-side over that page so the profile surface stays one
// read per tab; priority and state labels follow the issues area's
// vocabulary, while an ungrouped tab is a single "All work items"
// section. The local renderer is replaced by the issues area's layout
// components once those slices land — the shapes below are the seam.

import type { IssueListItem } from "@pidash/api-client";

import type { ProfileStateGroup } from "./stats.js";

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
const STATE_GROUP_TITLES: Record<ProfileStateGroup, string> = {
  backlog: "Backlog",
  unstarted: "Unstarted",
  started: "Started",
  review: "Review",
  test: "Test",
  completed: "Completed",
  cancelled: "Canceled",
};

/** An issue row's state group; absent state reads as backlog. */
export function issueStateGroup(
  issue: IssueListItem,
  groups: ReadonlyMap<string, ProfileStateGroup>
): ProfileStateGroup {
  if (issue.state_id === null) return "backlog";
  return groups.get(issue.state_id) ?? "backlog";
}

/** Member ids an issue carries; empty for unassigned rows. */
export function issueAssigneeIds(issue: IssueListItem): string[] {
  return issue.assignee_ids;
}

function groupTitle(
  dimension: ProfileGroupDimension,
  key: string,
  stateGroups: ReadonlyMap<string, ProfileStateGroup>,
  memberNames: ReadonlyMap<string, string>
): string {
  switch (dimension) {
    case "priority":
      return priorityLabel(key);
    case "state":
      return STATE_GROUP_TITLES[(stateGroups.get(key) as ProfileStateGroup | undefined) ?? "backlog"];
    case "assignees":
      return key === "unassigned" ? "Unassigned" : (memberNames.get(key) ?? key);
  }
}

/**
 * Group one page of issues. Rows land in the first matching value of the
 * dimension; unassigned rows group under "Unassigned"; ungrouped pages
 * are one "All work items" section. Empty pages answer an empty array.
 */
export function groupIssueRows(
  rows: readonly IssueListItem[],
  dimension: ProfileGroupDimension | null,
  stateGroups: ReadonlyMap<string, ProfileStateGroup> = new Map(),
  memberNames: ReadonlyMap<string, string> = new Map()
): ProfileIssueGroup[] {
  if (rows.length === 0) return [];
  if (dimension === null) return [{ id: "all", title: UNGROUPED_TITLE, rows: [...rows] }];
  const groups = new Map<string, IssueListItem[]>();
  for (const row of rows) {
    const keys =
      dimension === "assignees"
        ? row.assignee_ids.length === 0
          ? ["unassigned"]
          : row.assignee_ids
        : [dimension === "priority" ? row.priority : (row.state_id ?? "backlog")];
    for (const key of keys) {
      const bucket = groups.get(key);
      if (bucket === undefined) groups.set(key, [row]);
      else bucket.push(row);
    }
  }
  return [...groups.entries()]
    .sort(([left], [right]) => compareGroupKeys(dimension, left, right, stateGroups))
    .map(([key, bucket]) => ({
      id: key,
      title: groupTitle(dimension, key, stateGroups, memberNames),
      rows: bucket,
    }));
}

function stateRank(key: string, stateGroups: ReadonlyMap<string, ProfileStateGroup>): number {
  const group = stateGroups.get(key) ?? "backlog";
  const index = (Object.keys(STATE_GROUP_TITLES) as ProfileStateGroup[]).indexOf(group);
  return index < 0 ? 0 : index;
}

function compareGroupKeys(
  dimension: ProfileGroupDimension,
  left: string,
  right: string,
  stateGroups: ReadonlyMap<string, ProfileStateGroup>
): number {
  if (dimension === "priority") {
    return PRIORITY_ORDER.indexOf(left as PriorityRank) - PRIORITY_ORDER.indexOf(right as PriorityRank);
  }
  if (dimension === "state") return stateRank(left, stateGroups) - stateRank(right, stateGroups);
  if (left === "unassigned") return -1;
  if (right === "unassigned") return 1;
  return left.localeCompare(right);
}
