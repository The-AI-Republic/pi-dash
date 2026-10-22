// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar row shaping (parity/specs/issues.md §13, ISS-153–159). Pure
// predicates behind the date, module, cycle and estimate rows: which rows
// the project features expose, when a due date reads overdue, and how a
// multi-module toggle becomes the bridge's add/remove pair. Keeping them
// here lets the row components stay thin and every branch testable.

import type { Issue, ProjectDetail } from "@pidash/api-client";

/** Project fields the sidebar's conditional rows read. */
export type SidebarFeatureProject = Pick<ProjectDetail, "module_view" | "cycle_view" | "estimate">;

/** State groups that never flag a due date: the work is already closed. */
const CLOSED_GROUPS: ReadonlySet<string> = new Set(["completed", "cancelled"]);

/** True when the project runs modules, so the Modules row renders. */
export function showsModuleRow(project: Pick<SidebarFeatureProject, "module_view">): boolean {
  return project.module_view;
}

/** True when the project runs cycles, so the Cycle row renders. */
export function showsCycleRow(project: Pick<SidebarFeatureProject, "cycle_view">): boolean {
  return project.cycle_view;
}

/** True when the project has an estimate system, so the Estimate row renders. */
export function showsEstimateRow(project: Pick<SidebarFeatureProject, "estimate">): boolean {
  return project.estimate !== null && project.estimate !== undefined;
}

/** Local calendar day of a moment as YYYY-MM-DD (not the UTC day). */
function localDay(now: Date): string {
  const year = now.getFullYear();
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const day = String(now.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

/**
 * A due date reads overdue while its day is today or earlier and the
 * item is still open. Closed states (completed, cancelled) never flag,
 * and a missing date or state never flags.
 */
export function isDueDateOverdue(
  targetDate: string | null | undefined,
  stateGroup: string | null | undefined,
  now: Date = new Date()
): boolean {
  if (targetDate === null || targetDate === undefined || targetDate === "") return false;
  if (stateGroup === null || stateGroup === undefined) return false;
  if (CLOSED_GROUPS.has(stateGroup)) return false;
  return targetDate <= localDay(now);
}

/** The date each calendar bounds: start never passes the due date, and vice versa. */
export function dateBounds(issue: Pick<Issue, "start_date" | "target_date">): {
  startMax: string | null;
  dueMin: string | null;
} {
  return {
    startMax: issue.target_date ?? null,
    dueMin: issue.start_date ?? null,
  };
}
