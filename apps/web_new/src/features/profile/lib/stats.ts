// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Overview composition (parity/specs/profile.md §1, PROF-003/004): the
// three count cards, the seven workload groups zero-filled from the
// state distribution, and the two chart series with their empty states.
// The payload omits groups with no rows, so every reader starts from the
// fixed group order and fills zeros — never from the payload's length.

import type { ProfilePriorityBucket, ProfileStateBucket, UserProfileStats } from "@pidash/api-client";

/** The server's seven state groups in display order. */
export const PROFILE_STATE_GROUPS = [
  "backlog",
  "unstarted",
  "started",
  "review",
  "test",
  "completed",
  "cancelled",
] as const;

export type ProfileStateGroup = (typeof PROFILE_STATE_GROUPS)[number];

/** Workload card labels, which differ from the chart labels. */
export const WORKLOAD_LABELS: Record<ProfileStateGroup, string> = {
  backlog: "Backlog",
  unstarted: "Not started",
  started: "Working on",
  review: "Review",
  test: "Test",
  completed: "Completed",
  cancelled: "Canceled",
};

/** State chart legend labels. */
export const STATE_CHART_LABELS: Record<ProfileStateGroup, string> = {
  backlog: "Backlog",
  unstarted: "Unstarted",
  started: "Started",
  review: "Review",
  test: "Test",
  completed: "Completed",
  cancelled: "Canceled",
};

export interface CountCard {
  title: string;
  value: string;
  /** Address child the card opens. */
  tab: "created" | "assigned" | "subscribed";
}

/** The three count cards in a fixed order. */
export function overviewCards(
  stats: Pick<UserProfileStats, "created_issues" | "assigned_issues" | "subscribed_issues">
): CountCard[] {
  return [
    { title: "Work items created", value: String(stats.created_issues), tab: "created" },
    { title: "Work items assigned", value: String(stats.assigned_issues), tab: "assigned" },
    { title: "Work items subscribed", value: String(stats.subscribed_issues), tab: "subscribed" },
  ];
}

function countByGroup(buckets: ProfileStateBucket[]): Map<string, number> {
  return new Map(buckets.map((bucket) => [bucket.state_group, bucket.state_count]));
}

/** All seven workload groups with absent groups as zero. */
export function workloadGroups(
  buckets: ProfileStateBucket[]
): Array<{ group: ProfileStateGroup; label: string; count: string }> {
  const counts = countByGroup(buckets);
  return PROFILE_STATE_GROUPS.map((group) => ({
    group,
    label: WORKLOAD_LABELS[group],
    count: String(counts.get(group) ?? 0),
  }));
}

/** True when the member has nothing assigned; both charts show empty states. */
export function chartsAreEmpty(
  stats: Pick<UserProfileStats, "assigned_issues" | "state_distribution" | "priority_distribution">
): boolean {
  return (
    stats.assigned_issues === 0 && stats.state_distribution.length === 0 && stats.priority_distribution.length === 0
  );
}

/** Priority chart categories, capitalised, in the payload's own order. */
export function priorityCategories(buckets: ProfilePriorityBucket[]): string[] {
  return [...buckets]
    .sort((left, right) => left.priority_order - right.priority_order)
    .map((bucket) => bucket.priority.charAt(0).toUpperCase() + bucket.priority.slice(1));
}

/** Priority series for the bar chart, ordered like the categories. */
export function prioritySeries(buckets: ProfilePriorityBucket[]): Array<{ label: string; count: number }> {
  return [...buckets]
    .sort((left, right) => left.priority_order - right.priority_order)
    .map((bucket) => ({
      label: bucket.priority.charAt(0).toUpperCase() + bucket.priority.slice(1),
      count: bucket.priority_count,
    }));
}

/**
 * State legend rows. A member with no assigned work gets an empty legend
 * (the chart's empty state) rather than seven zero rows.
 */
export function stateLegend(
  buckets: ProfileStateBucket[]
): Array<{ group: ProfileStateGroup; label: string; count: string }> {
  if (buckets.length === 0) return [];
  const counts = countByGroup(buckets);
  return PROFILE_STATE_GROUPS.map((group) => ({
    group,
    label: STATE_CHART_LABELS[group],
    count: String(counts.get(group) ?? 0),
  }));
}

/** Largest slice for scaling the priority bars; zero when there is none. */
export function priorityMax(series: Array<{ count: number }>): number {
  return series.reduce((max, row) => Math.max(max, row.count), 0);
}

/** Largest slice for scaling the state bar; zero when there is none. */
export function stateTotal(buckets: ProfileStateBucket[]): number {
  return buckets.reduce((total, bucket) => total + bucket.state_count, 0);
}
