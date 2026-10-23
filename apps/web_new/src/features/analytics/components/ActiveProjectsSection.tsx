// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics active-projects section (parity/specs/analytics.md §7,
// ANL-007). Lists one row per project-stats row that resolves in
// the loaded project list: the project mark, a truncating name
// with its full form on hover, and a completion badge whose tone
// flips above the halfway mark. Placeholder rows cover the fetch;
// an empty or failed read shows the header with an empty list.
import { Badge, Skeleton, Tooltip } from "@pidash/kit";
import * as React from "react";

import { ProjectMark } from "../../../shared/pickers/ProjectMark.js";
import { useAnalyticsProjectStats } from "../api/projectStats.js";
import {
  completionPercent,
  completionTone,
  resolveActiveProjects,
  type ActiveProjectRow,
  type ActiveProjectSource,
} from "../lib/activeProjects.js";

export interface ActiveProjectsSectionProps {
  workspaceSlug: string;
  /** Resolved picker scope; enters the query key and the request. */
  projectIds: readonly string[];
  /** Selectable projects, joined to the stats rows. */
  projects: readonly ActiveProjectSource[];
}

/** Placeholder rows while the stats read flies (spec §7). */
const ACTIVE_PLACEHOLDER_ROWS = 5;

function ActiveRow({ row }: { row: ActiveProjectRow }): React.ReactElement {
  const percent = completionPercent(row.total, row.completed);
  const tone = completionTone(percent);
  return (
    <div data-testid="analytics-active-row" className="flex w-full items-center gap-(--space-3)">
      <ProjectMark logoProps={row.logoProps} identifier={row.identifier} size={28} />
      <Tooltip content={row.name}>
        <span
          data-testid="analytics-active-name"
          data-project-name={row.name}
          className="text-body min-w-0 flex-1 truncate font-medium text-(--text)"
        >
          {row.name}
        </span>
      </Tooltip>
      <span data-testid="analytics-active-badge" data-tone={tone}>
        <Badge tone={tone}>{`${percent}%`}</Badge>
      </span>
    </div>
  );
}

export function ActiveProjectsSection({
  workspaceSlug,
  projectIds,
  projects,
}: ActiveProjectsSectionProps): React.ReactElement {
  const stats = useAnalyticsProjectStats(workspaceSlug, projectIds);
  return (
    <section aria-labelledby="analytics-active-heading" data-testid="analytics-active">
      <h2 id="analytics-active-heading" className="text-h3 text-(--text)">
        Active Projects
      </h2>
      {stats.isPending ? (
        <div className="flex flex-col gap-(--space-3)">
          {Array.from({ length: ACTIVE_PLACEHOLDER_ROWS }, (_, index) => (
            <div key={index} data-testid="analytics-active-placeholder" className="flex items-center gap-(--space-3)">
              <Skeleton width="28px" height="28px" />
              <Skeleton height="14px" />
              <Skeleton width="48px" height="20px" />
            </div>
          ))}
        </div>
      ) : (
        <div
          data-testid="analytics-active-list"
          className="flex max-h-[320px] flex-col gap-(--space-3) overflow-y-auto"
        >
          {resolveActiveProjects(stats.data ?? [], projects).map((row) => (
            <ActiveRow key={row.id} row={row} />
          ))}
        </div>
      )}
    </section>
  );
}
