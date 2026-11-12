// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace state-breakdown table (parity/specs/analytics.md §12,
// ANL-013). One row per project with its five workflow-group counts.
// The read covers five skeleton rows, and an empty or failed response
// keeps the real headers and guidance with a zero count. Search and
// export are client-side over the server-ordered rows.
import type { Project } from "@pidash/api-client";
import * as React from "react";

import { analyticsCsvColumns, buildAnalyticsCsv, downloadCsvFile } from "../../../shared/export/csv.js";
import { ProjectMarkIcon } from "../../../shared/pickers/ProjectMarkIcon.js";
import { projectMark } from "../../../shared/pickers/projectIcon.js";
import { useAnalyticsStats } from "../api/stats.js";
import { breakdownRows, breakdownTableColumns, type BreakdownProject } from "../lib/table.js";
import { AnalyticsTable } from "./AnalyticsTable.js";

export interface StateBreakdownSectionProps {
  workspaceSlug: string;
  projectIds: readonly string[];
  /** Joined projects used to resolve the live name and mark. */
  projects: readonly Project[];
}

/** Guidance copy for an empty or failed stats read (oracle-pinned). */
const BREAKDOWN_GUIDANCE =
  "Create projects with work items and assignees to start tracking performance, progress, and team impact here.";

/** Skeleton body row count (spec §12). */
const BREAKDOWN_SKELETON_ROWS = 5;

/** Decorative project identity; never contributes text to the cell. */
function BreakdownMark({
  logoProps,
  identifier,
}: {
  logoProps: Record<string, unknown>;
  identifier: string;
}): React.ReactElement {
  const mark = projectMark(logoProps);
  return (
    <span
      aria-hidden="true"
      data-testid="breakdown-project-mark"
      className="flex size-5 shrink-0 items-center justify-center overflow-hidden rounded-(--radius-control) bg-(--subtle)"
    >
      {mark.kind === "image" ? (
        <img src={mark.url} alt="" className="size-full object-cover" />
      ) : mark.kind === "icon" ? (
        <ProjectMarkIcon mark={mark} identifier={identifier} size={14} />
      ) : null}
    </span>
  );
}

export function StateBreakdownSection({
  workspaceSlug,
  projectIds,
  projects,
}: StateBreakdownSectionProps): React.ReactElement {
  const stats = useAnalyticsStats(workspaceSlug, projectIds);
  const columns = React.useMemo(() => breakdownTableColumns(), []);
  const projectRows = React.useMemo<BreakdownProject[]>(
    () =>
      projects.map((project) => ({
        id: project.id,
        name: project.name,
        identifier: project.identifier,
        logo_props: project.logo_props,
      })),
    [projects]
  );
  const rows = React.useMemo(() => breakdownRows(stats.data ?? [], projectRows), [stats.data, projectRows]);
  const loading = stats.isPending;

  return (
    <section aria-labelledby="analytics-breakdown-heading" data-testid="analytics-breakdown">
      <h2 id="analytics-breakdown-heading" className="text-h3 mb-(--space-3) text-(--text)">
        State breakdown
      </h2>
      <AnalyticsTable
        testId="analytics-breakdown-table"
        columns={columns.map((column) =>
          column.key === "project"
            ? {
                ...column,
                render: (row: (typeof rows)[number]) =>
                  row.project === undefined ? (
                    <span className="inline-flex items-center gap-(--space-2)">
                      <span
                        aria-hidden="true"
                        data-testid="breakdown-project-mark"
                        className="size-5 shrink-0 rounded-(--radius-control) bg-(--subtle)"
                      />
                    </span>
                  ) : (
                    <span className="inline-flex items-center gap-(--space-2)">
                      <BreakdownMark logoProps={row.project.logo_props} identifier={row.project.identifier} />
                      <span>{row.project.name}</span>
                    </span>
                  ),
              }
            : column
        )}
        rows={rows}
        countNoun="Projects"
        loading={loading}
        skeletonRows={BREAKDOWN_SKELETON_ROWS}
        emptyText={BREAKDOWN_GUIDANCE}
        onExport={(visible) => {
          downloadCsvFile(buildAnalyticsCsv(workspaceSlug, analyticsCsvColumns(columns), visible));
        }}
      />
    </section>
  );
}
