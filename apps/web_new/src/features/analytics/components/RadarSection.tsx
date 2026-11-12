// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics project-insights section (parity/specs/analytics.md §6,
// ANL-006). Pairs the per-project radar chart with a written summary
// that repeats the dimensions with their counts; both read the same
// server payload in server order, zero rows preserved. A skeleton
// covers the fetch, a guidance panel covers a successful-but-empty
// read, and a failed read keeps the headers with neither plot,
// summary rows, nor guidance.
import { Skeleton } from "@pidash/kit";
import * as React from "react";

import { RadarChart } from "../../../shared/charts/index.js";
import { useAnalyticsProjectsChart } from "../api/charts.js";
import { selectPickerSummary } from "../lib/scope.js";
import { AnalyticsSectionGuidance } from "./AnalyticsSectionGuidance.js";

/** Selectable project identity for the scope row. */
export interface RadarScopeProject {
  id: string;
  name: string;
}

export interface RadarSectionProps {
  workspaceSlug: string;
  /** Resolved picker scope; enters the query key and the request. */
  projectIds: readonly string[];
  /** Selectable projects, for the scope row's names. */
  projects: readonly RadarScopeProject[];
}

/** Radar series name (spec §6). */
const RADAR_SERIES_NAME = "Count";

function RadarShell({ children }: { children: React.ReactNode }): React.ReactElement {
  return (
    <section aria-labelledby="analytics-radar-heading" data-testid="analytics-radar">
      <h2 id="analytics-radar-heading" className="text-h3 text-(--text)">
        Project Insights
      </h2>
      {children}
    </section>
  );
}

function SummaryChrome({ scopeLabel }: { scopeLabel: string }): React.ReactElement {
  return (
    <div>
      <h3 className="text-emphasis text-(--text)">Project summary</h3>
      <div data-testid="analytics-radar-scope" className="text-body text-(--text-muted)">
        {scopeLabel}
      </div>
    </div>
  );
}

export function RadarSection({ workspaceSlug, projectIds, projects }: RadarSectionProps): React.ReactElement {
  const graph = useAnalyticsProjectsChart(workspaceSlug, projectIds);
  const byId = new Map(projects.map((project) => [project.id, project.name] as const));
  const scopeNames = projectIds.flatMap((id) => {
    const name = byId.get(id);
    return name === undefined ? [] : [name];
  });
  const scopeLabel = selectPickerSummary(scopeNames);
  const rows = graph.data ?? [];

  if (graph.isPending) {
    return (
      <RadarShell>
        <div data-testid="analytics-radar-skeleton" className="grid gap-(--space-4)">
          <Skeleton height="280px" />
          <Skeleton height="12px" />
          <Skeleton height="12px" />
          <Skeleton height="12px" />
        </div>
      </RadarShell>
    );
  }

  if (graph.isError) {
    return (
      <RadarShell>
        <SummaryChrome scopeLabel={scopeLabel} />
      </RadarShell>
    );
  }

  if (rows.length === 0) {
    return (
      <RadarShell>
        <AnalyticsSectionGuidance
          title="Nothing to plot yet"
          description="Add work items and assignees to projects to see performance charted here."
        />
      </RadarShell>
    );
  }

  return (
    <RadarShell>
      <div className="grid gap-(--space-6) md:grid-cols-2">
        <RadarChart
          points={rows.map((row) => ({ label: row.name, value: row.count }))}
          seriesName={RADAR_SERIES_NAME}
        />
        <div className="flex flex-col gap-(--space-3)">
          <SummaryChrome scopeLabel={scopeLabel} />
          <ul className="flex flex-col gap-(--space-2)">
            {rows.map((row) => (
              <li
                key={row.key}
                data-testid="analytics-radar-row"
                className="flex items-center justify-between gap-(--space-4)"
              >
                <span data-testid="analytics-radar-dimension" className="text-body text-(--text)">
                  {row.name}
                </span>
                <span data-testid="analytics-radar-count" className="text-body text-(--text-muted)">
                  {row.count}
                </span>
              </li>
            ))}
          </ul>
        </div>
      </div>
    </RadarShell>
  );
}
