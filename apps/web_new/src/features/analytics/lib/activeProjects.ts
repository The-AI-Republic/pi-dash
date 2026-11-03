// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Active-projects list rules (parity/specs/analytics.md §7,
// ANL-007). Pure: completion math, badge tone, and the join of
// project-stats rows to the loaded project list. Components build
// on these; nothing here touches React, the router, or the network.
import type { Project, ProjectStatsRow } from "@pidash/api-client";

/** Project fields the active-projects join reads. */
export type ActiveProjectSource = Pick<
  Project,
  "id" | "name" | "identifier" | "logo_props" | "member_role" | "archived_at"
>;

/**
 * Completion percent for one row: zero without a positive total,
 * otherwise the rounded share. Missing counts read as zero (the
 * contract's defensive tolerance).
 */
export function completionPercent(total: number | null | undefined, completed: number | null | undefined): number {
  if (total === null || total === undefined || total <= 0) return 0;
  const done = completed ?? 0;
  if (done <= 0) return 0;
  return Math.round((done / total) * 100);
}

/** Badge tone: success only strictly above the halfway mark. */
export type CompletionTone = "success" | "danger";

export function completionTone(percent: number): CompletionTone {
  return percent > 50 ? "success" : "danger";
}

/** One rendered row: project identity plus its completion counts. */
export interface ActiveProjectRow {
  id: string;
  name: string;
  identifier: string;
  logoProps: Record<string, unknown>;
  total: number;
  completed: number;
}

/**
 * Stats rows joined to the loaded project list in server order.
 * Rows whose id no longer resolves render nothing. Counts read
 * null or absent as zero.
 */
export function resolveActiveProjects(
  stats: readonly ProjectStatsRow[],
  projects: readonly ActiveProjectSource[]
): ActiveProjectRow[] {
  const byId = new Map(projects.map((project) => [project.id, project] as const));
  return stats.flatMap((row) => {
    const project = byId.get(row.id);
    if (project === undefined) return [];
    return [
      {
        id: project.id,
        name: project.name,
        identifier: project.identifier,
        logoProps: project.logo_props,
        total: row.total_issues ?? 0,
        completed: row.completed_issues ?? 0,
      },
    ];
  });
}
