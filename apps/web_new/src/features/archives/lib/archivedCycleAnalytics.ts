// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-cycle analytics selectors (parity/specs/archives.md §5.6,
// ARCH-025). The archived detail read carries the same summaries the
// live sidebar shows — the group counts, the assignee and label
// distributions and the burndown series — but the per-group estimate
// points the live progress snapshot carries are absent, and the
// distribution rows arrive as string-keyed records. These selectors
// coerce the archived shapes into the live row shapes for the issues
// measure only, so the peek renders the same summaries read-only. A
// points measure would need per-group estimate points the archived
// record does not carry, so the peek never offers one.

import type { ArchivedCycleDetail, CycleCompletionChart, CycleProgress } from "@pidash/api-client";

import { progressShare, selectStateGroupRows, type CycleDistributionRow } from "../../cycles/index.js";

/** Fallback title of the unassigned distribution row. */
export const ARCHIVED_CYCLE_NO_ASSIGNEE_TITLE = "Unassigned";

/** Fallback title of an unnamed label distribution row. */
export const ARCHIVED_CYCLE_NO_LABEL_TITLE = "Unlabeled";

/**
 * The live progress snapshot behind the states breakdown, rebuilt
 * from the archived detail's own counts. The per-group estimate
 * points have no archived source and stay zero; the issues measure
 * never reads them.
 */
export function archivedCycleProgress(cycle: ArchivedCycleDetail): CycleProgress {
  return {
    total_issues: cycle.total_issues,
    backlog_issues: cycle.backlog_issues,
    unstarted_issues: cycle.unstarted_issues,
    started_issues: cycle.started_issues,
    completed_issues: cycle.completed_issues,
    cancelled_issues: cycle.cancelled_issues,
    backlog_estimate_points: 0,
    unstarted_estimate_points: 0,
    started_estimate_points: 0,
    completed_estimate_points: cycle.completed_estimate_points,
    cancelled_estimate_points: 0,
    total_estimate_points: cycle.total_estimate_points,
  };
}

/** State-group rows for the issues measure, in live order. */
export function selectArchivedCycleStateRows(cycle: ArchivedCycleDetail): CycleDistributionRow[] {
  return selectStateGroupRows(archivedCycleProgress(cycle), "issues").map((row) => ({
    id: row.key,
    title: row.title,
    count: row.count,
    total: row.total,
    percent: row.percent,
  }));
}

function asCount(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function asId(value: unknown): string | null {
  return typeof value === "string" && value !== "" ? value : null;
}

function asTitle(value: unknown, fallback: string): string {
  return typeof value === "string" && value !== "" ? value : fallback;
}

function distributionRecords(distribution: unknown): ReadonlyArray<Record<string, unknown>> {
  if (!Array.isArray(distribution)) return [];
  return distribution.filter((entry): entry is Record<string, unknown> => typeof entry === "object" && entry !== null);
}

/** Assignee rows for the issues measure, in server order. */
export function selectArchivedCycleAssigneeRows(cycle: ArchivedCycleDetail): CycleDistributionRow[] {
  const total = cycle.total_issues;
  return distributionRecords(cycle.distribution.assignees).map((entry) => {
    const count = asCount(entry["total_issues"]);
    return {
      id: asId(entry["assignee_id"]),
      title: asTitle(entry["display_name"], ARCHIVED_CYCLE_NO_ASSIGNEE_TITLE),
      count,
      total,
      percent: progressShare(count, total),
    };
  });
}

/** Label rows for the issues measure, in server order. */
export function selectArchivedCycleLabelRows(cycle: ArchivedCycleDetail): CycleDistributionRow[] {
  const total = cycle.total_issues;
  return distributionRecords(cycle.distribution.labels).map((entry) => {
    const count = asCount(entry["total_issues"]);
    return {
      id: asId(entry["label_id"]),
      title: asTitle(entry["label_name"], ARCHIVED_CYCLE_NO_LABEL_TITLE),
      count,
      total,
      percent: progressShare(count, total),
    };
  });
}

/**
 * The burndown series as the live chart reads it, or null when the
 * archived record carries no chart. Entries that are neither numbers
 * nor nulls are skipped the way the live points parser skips them.
 */
export function selectArchivedCycleChart(cycle: ArchivedCycleDetail): CycleCompletionChart | null {
  const chart = cycle.distribution.completion_chart;
  if (typeof chart !== "object" || chart === null || Array.isArray(chart)) return null;
  const out: Record<string, number | null> = {};
  for (const [date, remaining] of Object.entries(chart)) {
    if (typeof remaining !== "number" && remaining !== null) continue;
    out[date] = remaining;
  }
  return out;
}
