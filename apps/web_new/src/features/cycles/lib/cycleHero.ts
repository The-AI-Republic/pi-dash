// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Active-cycle hero models (parity/specs/cycles.md §15, CYC-041):
// the progress groups with their closed summary, the burn-down
// series per measure, and the breakdown entries the hero tabs list.
// Pure so the cards and the sidebar share the arithmetic.

import type { CycleDistribution, CycleProgress, IssueListItem } from "@pidash/api-client";

/** Issues count work items; points sum their estimates. */
export type HeroMeasure = "issues" | "points";

/** Progress groups in lifecycle order. */
export type HeroProgressGroupId = "backlog" | "unstarted" | "started" | "completed" | "cancelled";

export interface HeroProgressGroup {
  id: HeroProgressGroupId;
  count: number;
}

const PROGRESS_GROUP_ORDER: HeroProgressGroupId[] = ["backlog", "unstarted", "started", "completed", "cancelled"];

/** All five groups in lifecycle order, zeros included, for a stable card. */
export function heroProgressGroups(
  progress: Pick<
    CycleProgress,
    "backlog_issues" | "unstarted_issues" | "started_issues" | "completed_issues" | "cancelled_issues"
  >
): HeroProgressGroup[] {
  const counts: Record<HeroProgressGroupId, number> = {
    backlog: progress.backlog_issues,
    unstarted: progress.unstarted_issues,
    started: progress.started_issues,
    completed: progress.completed_issues,
    cancelled: progress.cancelled_issues,
  };
  return PROGRESS_GROUP_ORDER.map((id) => ({ id, count: counts[id] }));
}

/** `{closed}/{openable} Work items closed`; the denominator drops cancelled. */
export function heroClosedSummary(
  progress: Pick<CycleProgress, "total_issues" | "completed_issues" | "cancelled_issues">
): string {
  const closed = progress.completed_issues + progress.cancelled_issues;
  const openable = Math.max(0, progress.total_issues - progress.cancelled_issues);
  return `${closed}/${openable} Work items closed`;
}

/** The cancelled count line under the closed summary. */
export function heroCancelledLine(cancelledIssues: number): string {
  return `Cancelled work items ${cancelledIssues}`;
}

/** Work still open: backlog plus unstarted plus started. */
export function heroPendingCount(
  progress: Pick<CycleProgress, "backlog_issues" | "unstarted_issues" | "started_issues">
): string {
  const pending = progress.backlog_issues + progress.unstarted_issues + progress.started_issues;
  return `Pending work items - ${pending}`;
}

export interface HeroBurnPoint {
  day: string;
  remaining: number;
}

/**
 * Chart days in ascending order with their remaining counts. The
 * distribution carries one remaining per day plus a `${day}_estimates`
 * twin; the points measure reads the twin and falls back to the day
 * count where the server sent none. Future days carry null so the
 * chart stops at today; a missing count still plots as zero.
 */
export function heroBurndownPoints(
  chart: Record<string, number | null | undefined>,
  measure: HeroMeasure
): HeroBurnPoint[] {
  const points: HeroBurnPoint[] = [];
  const days = Object.keys(chart)
    .filter((day) => !day.endsWith("_estimates"))
    .sort();
  for (const day of days) {
    const twin = measure === "points" ? chart[`${day}_estimates`] : undefined;
    const remaining = twin ?? chart[day];
    if (remaining === null) continue;
    points.push({ day, remaining: remaining ?? 0 });
  }
  return points;
}

/**
 * The chart plots only for a dated cycle that has started: undated
 * and future-start cycles show the chart placeholder instead.
 */
export function heroChartAvailable(
  startDate: string | null | undefined,
  endDate: string | null | undefined,
  now: Date = new Date()
): boolean {
  if (startDate === null || startDate === undefined || endDate === null || endDate === undefined) return false;
  const start = new Date(startDate).getTime();
  if (Number.isNaN(start)) return false;
  return start <= now.getTime();
}

export interface HeroAssigneeEntry {
  id: string;
  name: string;
  avatar: string | null;
  count: number;
}

/** Assignee rows: display names with issue or estimate counts. Sparse server rows fall back to blanks. */
export function heroAssigneeEntries(distribution: CycleDistribution, measure: HeroMeasure): HeroAssigneeEntry[] {
  return distribution.assignees.map((row) => ({
    id: row.assignee_id ?? "",
    name: row.display_name ?? "",
    avatar: row.avatar_url ?? null,
    count: (measure === "points" ? row.total_estimates : row.total_issues) ?? 0,
  }));
}

export interface HeroLabelEntry {
  id: string;
  name: string;
  color: string | null;
  count: number;
}

/** Label rows: names with dots and issue or estimate counts. Sparse server rows fall back to blanks. */
export function heroLabelEntries(distribution: CycleDistribution, measure: HeroMeasure): HeroLabelEntry[] {
  return distribution.labels.map((row) => ({
    id: row.label_id ?? "",
    name: row.label_name ?? "",
    color: row.color ?? null,
    count: (measure === "points" ? row.total_estimates : row.total_issues) ?? 0,
  }));
}

export interface HeroPriorityEntry {
  id: string;
  name: string;
  priority: string;
}

/** The urgent and high rows of the first issues page, in server order. */
export function heroPriorityEntries(
  issues: readonly Pick<IssueListItem, "id" | "name" | "priority">[]
): HeroPriorityEntry[] {
  return issues
    .filter((issue) => issue.priority === "urgent" || issue.priority === "high")
    .map((issue) => ({ id: issue.id, name: issue.name, priority: issue.priority }));
}

/** Breakdown tabs in pinned order with their exact labels. */
export type HeroBreakdownTab = "priority" | "assignees" | "labels";

export const HERO_BREAKDOWN_TABS: ReadonlyArray<{ id: HeroBreakdownTab; label: string }> = [
  { id: "priority", label: "Priority work items" },
  { id: "assignees", label: "Assignees" },
  { id: "labels", label: "Labels" },
];

/** Per-cycle tab memory; the sidebar keeps its own separate key. */
export function heroTabStorageKey(cycleId: string): string {
  return `cycle-hero-tab-${cycleId}`;
}

/** A stored tab id, or the priority tab when nothing valid was kept. */
export function parseHeroTab(value: string | null | undefined): HeroBreakdownTab {
  if (value === "priority" || value === "assignees" || value === "labels") return value;
  return "priority";
}
