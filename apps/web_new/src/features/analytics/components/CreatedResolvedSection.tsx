// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics created-vs-resolved section (parity/specs/analytics.md
// §9, ANL-009). Plots the work-items buckets as two smoothed area
// series — resolved first, then created — with calendar axis
// labels, a bottom legend, and a hover tooltip. A skeleton covers
// the fetch; a failed or empty read shows the guidance panel with
// no plot, legend, or axis labels.
import { Skeleton } from "@pidash/kit";
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { useResolvedScheme } from "../../../core/theme/index.js";
import { AreaChart, resolveChartScheme, type AreaSeriesDef } from "../../../shared/charts/index.js";
import { getAppClient } from "../../../core/api/client.js";
import { analyticsWorkItemsChartQueryOptions } from "../api/charts.js";
import type { AnalyticsDialogScope } from "../api/keys.js";
import { dialogWorkItemsChartQueryOptions } from "../api/projectScope.js";
import {
  CREATED_SERIES_KEY,
  CREATED_SERIES_NAME,
  RESOLVED_SERIES_KEY,
  RESOLVED_SERIES_NAME,
  selectCreatedResolvedColors,
  selectCreatedResolvedPoints,
} from "../lib/createdResolved.js";
import { AnalyticsSectionGuidance } from "./AnalyticsSectionGuidance.js";

export interface CreatedResolvedSectionProps {
  workspaceSlug: string;
  /** Resolved picker scope; enters the query key and the request. */
  projectIds: readonly string[];
  /** Scoped dialog mode (ANL-014): read the project endpoint instead. */
  dialogScope?: AnalyticsDialogScope | undefined;
}

/** Guidance copy for a failed or empty series read (oracle-pinned). */
const CREATED_RESOLVED_GUIDANCE =
  "Create projects with work items and assignees to start tracking performance, progress, and team impact here.";

function CreatedResolvedShell({ children }: { children: React.ReactNode }): React.ReactElement {
  return (
    <section aria-labelledby="analytics-created-heading" data-testid="analytics-created">
      <h2 id="analytics-created-heading" className="text-h3 text-(--text)">
        Created vs Resolved
      </h2>
      {children}
    </section>
  );
}

export function CreatedResolvedSection({
  workspaceSlug,
  projectIds,
  dialogScope,
}: CreatedResolvedSectionProps): React.ReactElement {
  // Exactly one source reads: the scoped dialog uses the project family,
  // the tab the workspace family. Both hooks mount so their order is fixed.
  const dialogSeries = useQuery({
    ...dialogWorkItemsChartQueryOptions(
      getAppClient(),
      workspaceSlug,
      dialogScope ?? { projectId: "", cycleId: null, moduleId: null }
    ),
    enabled: dialogScope !== undefined,
  });
  const tabSeries = useQuery({
    ...analyticsWorkItemsChartQueryOptions(getAppClient(), workspaceSlug, projectIds),
    enabled: dialogScope === undefined,
  });
  const series = dialogScope === undefined ? tabSeries : dialogSeries;
  const { scheme, customDark } = useResolvedScheme();

  if (series.isPending) {
    return (
      <CreatedResolvedShell>
        <div data-testid="analytics-created-skeleton">
          <Skeleton height="320px" />
        </div>
      </CreatedResolvedShell>
    );
  }

  const rows = series.data?.data ?? [];
  if (series.isError || rows.length === 0) {
    return (
      <CreatedResolvedShell>
        <AnalyticsSectionGuidance title={CREATED_RESOLVED_GUIDANCE} description="" />
      </CreatedResolvedShell>
    );
  }

  const colors = selectCreatedResolvedColors(resolveChartScheme(scheme, customDark));
  const plotted: readonly AreaSeriesDef[] = [
    { key: RESOLVED_SERIES_KEY, name: RESOLVED_SERIES_NAME, color: colors.resolved },
    { key: CREATED_SERIES_KEY, name: CREATED_SERIES_NAME, color: colors.created },
  ];
  return (
    <CreatedResolvedShell>
      <AreaChart points={selectCreatedResolvedPoints(rows)} series={plotted} />
    </CreatedResolvedShell>
  );
}
