// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics custom-insights section (parity/specs/analytics.md
// §10, ANL-010, ANL-011). Pairs the metric, x-axis, and split
// pickers with the custom bar chart. Every re-pick re-queries the
// custom graph; without a split the chart renders one series with
// per-bar fills, with a split it stacks one palette segment per
// schema key. A skeleton covers loading, including the state
// reference read behind state axes; a failed or empty read shows
// the guidance panel with no chart. Picker state is local to the
// panel.
import type { AnalyticsXAxis, AnalyticsYAxis } from "@pidash/api-client";
import { Skeleton } from "@pidash/kit";
import * as React from "react";

import { useResolvedScheme } from "../../../core/theme/index.js";
import { BarChart, resolveChartScheme } from "../../../shared/charts/index.js";
import { usePickerWorkspaceStates } from "../../../shared/pickers/reference.js";
import { useAnalyticsCustomChart } from "../api/charts.js";
import { DEFAULT_CUSTOM_METRIC, DEFAULT_CUSTOM_SPLIT, DEFAULT_CUSTOM_X_AXIS } from "../lib/customAxes.js";
import { buildCustomChartModel } from "../lib/customChart.js";
import { AnalyticsSectionGuidance } from "./AnalyticsSectionGuidance.js";
import { CustomInsightPickers } from "./CustomInsightPickers.js";

export interface CustomInsightsSectionProps {
  workspaceSlug: string;
  /** Resolved picker scope; enters the query key and the request. */
  projectIds: readonly string[];
}

/** Guidance copy for a failed or empty custom read (oracle-pinned). */
const CUSTOM_INSIGHTS_GUIDANCE =
  "Create projects with work items and assignees to start tracking performance, progress, and team impact here.";

export function CustomInsightsSection({ workspaceSlug, projectIds }: CustomInsightsSectionProps): React.ReactElement {
  const [metric, setMetric] = React.useState<AnalyticsYAxis>(DEFAULT_CUSTOM_METRIC);
  const [xAxis, setXAxis] = React.useState<AnalyticsXAxis>(DEFAULT_CUSTOM_X_AXIS);
  const [split, setSplit] = React.useState<AnalyticsXAxis | null>(DEFAULT_CUSTOM_SPLIT);
  const graph = useAnalyticsCustomChart(workspaceSlug, projectIds, {
    xAxis,
    yAxis: metric,
    groupBy: split ?? undefined,
  });
  const needsStates = xAxis === "STATES" && split === null;
  const states = usePickerWorkspaceStates(workspaceSlug, needsStates);
  const { scheme, customDark } = useResolvedScheme();

  const loading = graph.isPending || (needsStates && states.isPending);
  const rows = graph.data?.data ?? [];
  const showGuidance = !loading && (graph.isError || rows.length === 0);

  const model = buildCustomChartModel({
    rows,
    schema: graph.data?.schema ?? {},
    xAxis,
    split,
    states: states.data ?? [],
    scheme: resolveChartScheme(scheme, customDark),
    currentYear: new Date().getFullYear(),
  });

  return (
    <section aria-labelledby="analytics-custom-heading" data-testid="analytics-custom">
      <h2 id="analytics-custom-heading" className="text-h3 text-(--text)">
        Custom Insights
      </h2>
      <CustomInsightPickers
        metric={metric}
        xAxis={xAxis}
        split={split}
        onMetricChange={setMetric}
        onXAxisChange={setXAxis}
        onSplitChange={setSplit}
      />
      {loading ? (
        <div data-testid="analytics-custom-skeleton">
          <Skeleton height="320px" />
        </div>
      ) : showGuidance ? (
        <AnalyticsSectionGuidance title={CUSTOM_INSIGHTS_GUIDANCE} description="" />
      ) : (
        <BarChart bars={model.bars} segments={model.segments} stacked={split !== null} fills={model.fills} />
      )}
    </section>
  );
}
