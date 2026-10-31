// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  ActiveProjectsSection,
  ALL_PROJECTS_LABEL,
  ANALYTICS_TAB_IDS,
  ANALYTICS_TAB_LABELS,
  AnalyticsEmptyView,
  AnalyticsPicker,
  AnalyticsScreen,
  AnalyticsSectionGuidance,
  AnalyticsTabStrip,
  analyticsCustomChartQueryOptions,
  analyticsKeys,
  analyticsProjectsChartQueryOptions,
  analyticsProjectStatsQueryOptions,
  analyticsStatsQueryOptions,
  AnalyticsTable,
  breakdownRows,
  breakdownTableColumns,
  customTableColumns,
  customTableRows,
  filterByFirstColumn,
  rowCountLabel,
  StateBreakdownSection,
  useAnalyticsStats,
  analyticsScopeSearch,
  analyticsTabHref,
  analyticsTotalsQueryOptions,
  analyticsWorkItemsChartQueryOptions,
  buildCustomChartModel,
  capitalizeCustomLabel,
  completionPercent,
  completionTone,
  CREATED_SERIES_NAME,
  CreatedResolvedSection,
  CUSTOM_AXES,
  CUSTOM_METRICS,
  CUSTOM_SINGLE_SERIES_NAME,
  CUSTOM_SPLIT_CLEAR_LABEL,
  CUSTOM_SPLIT_EMPTY_LABEL,
  customAxisLabel,
  CustomInsightPickers,
  CustomInsightsSection,
  customMetricLabel,
  DEFAULT_ANALYTICS_TAB,
  DEFAULT_CUSTOM_METRIC,
  DEFAULT_CUSTOM_SPLIT,
  DEFAULT_CUSTOM_X_AXIS,
  defaultAnalyticsTabHref,
  formatCustomDateLabel,
  formatSeriesMonth,
  HeadlineCards,
  headlineCardsForTab,
  indexStateColors,
  isAnalyticsTabId,
  isCapitalizedCustomAxis,
  isCustomDateAxis,
  MANY_PROJECTS_LABEL,
  normalizeAnalyticsScope,
  OVERVIEW_HEADLINE_CARDS,
  parseAnalyticsSearch,
  RadarSection,
  RESOLVED_SERIES_NAME,
  resolveActiveProjects,
  resolveAnalyticsScope,
  selectAnalyticsProjects,
  selectAnalyticsTab,
  selectCreatedResolvedColors,
  selectCreatedResolvedPoints,
  selectCustomBarLabel,
  selectCustomSegmentLabel,
  selectCustomSplitOptions,
  selectCustomXOptions,
  selectHeadlineValue,
  selectPickerSummary,
  selectPriorityBarColor,
  selectStateBarColor,
  useAnalyticsCustomChart,
  useAnalyticsProjectsChart,
  useAnalyticsProjectStats,
  useAnalyticsTotals,
  useAnalyticsWorkItemsChart,
  WORK_ITEMS_HEADLINE_CARDS,
} from "./index.js";

describe("analytics barrel", () => {
  it("exposes the feature's public API", () => {
    expect(typeof AnalyticsEmptyView).toBe("function");
    expect(typeof AnalyticsPicker).toBe("function");
    expect(typeof AnalyticsScreen).toBe("function");
    expect(typeof AnalyticsTabStrip).toBe("function");
    expect(typeof HeadlineCards).toBe("function");
    expect(typeof useAnalyticsTotals).toBe("function");
    expect(typeof analyticsTotalsQueryOptions).toBe("function");
    expect(typeof analyticsProjectsChartQueryOptions).toBe("function");
    expect(typeof analyticsProjectStatsQueryOptions).toBe("function");
    expect(typeof analyticsStatsQueryOptions).toBe("function");
    expect(typeof useAnalyticsStats).toBe("function");
    expect(typeof AnalyticsTable).toBe("function");
    expect(typeof StateBreakdownSection).toBe("function");
    expect(typeof analyticsKeys.stats).toBe("function");
    expect(typeof filterByFirstColumn).toBe("function");
    expect(typeof rowCountLabel).toBe("function");
    expect(typeof customTableColumns).toBe("function");
    expect(typeof customTableRows).toBe("function");
    expect(typeof breakdownTableColumns).toBe("function");
    expect(typeof breakdownRows).toBe("function");
    expect(typeof useAnalyticsProjectsChart).toBe("function");
    expect(typeof useAnalyticsProjectStats).toBe("function");
    expect(typeof analyticsWorkItemsChartQueryOptions).toBe("function");
    expect(typeof useAnalyticsWorkItemsChart).toBe("function");
    expect(typeof analyticsCustomChartQueryOptions).toBe("function");
    expect(typeof useAnalyticsCustomChart).toBe("function");
    expect(typeof RadarSection).toBe("function");
    expect(typeof ActiveProjectsSection).toBe("function");
    expect(typeof CreatedResolvedSection).toBe("function");
    expect(typeof CustomInsightPickers).toBe("function");
    expect(typeof CustomInsightsSection).toBe("function");
    expect(typeof AnalyticsSectionGuidance).toBe("function");
    expect(typeof completionPercent).toBe("function");
    expect(typeof completionTone).toBe("function");
    expect(typeof resolveActiveProjects).toBe("function");
    expect(typeof analyticsKeys.totals).toBe("function");
    expect(typeof analyticsKeys.charts).toBe("function");
    expect(typeof analyticsKeys.customCharts).toBe("function");
    expect(typeof analyticsKeys.projectStats).toBe("function");
    expect(typeof formatSeriesMonth).toBe("function");
    expect(typeof selectCreatedResolvedColors).toBe("function");
    expect(typeof selectCreatedResolvedPoints).toBe("function");
    expect(typeof customAxisLabel).toBe("function");
    expect(typeof customMetricLabel).toBe("function");
    expect(typeof isCustomDateAxis).toBe("function");
    expect(typeof isCapitalizedCustomAxis).toBe("function");
    expect(typeof selectCustomXOptions).toBe("function");
    expect(typeof selectCustomSplitOptions).toBe("function");
    expect(typeof buildCustomChartModel).toBe("function");
    expect(typeof capitalizeCustomLabel).toBe("function");
    expect(typeof formatCustomDateLabel).toBe("function");
    expect(typeof indexStateColors).toBe("function");
    expect(typeof selectCustomBarLabel).toBe("function");
    expect(typeof selectCustomSegmentLabel).toBe("function");
    expect(typeof selectPriorityBarColor).toBe("function");
    expect(typeof selectStateBarColor).toBe("function");
    expect(CUSTOM_AXES.length).toBe(12);
    expect(CUSTOM_METRICS.length).toBe(1);
    expect(CUSTOM_SPLIT_CLEAR_LABEL).toBe("No value");
    expect(CUSTOM_SPLIT_EMPTY_LABEL).toBe("Add Property");
    expect(DEFAULT_CUSTOM_METRIC).toBe("WORK_ITEM_COUNT");
    expect(DEFAULT_CUSTOM_X_AXIS).toBe("PRIORITY");
    expect(DEFAULT_CUSTOM_SPLIT).toBeNull();
    expect(CUSTOM_SINGLE_SERIES_NAME).toBe("Count");
    expect(RESOLVED_SERIES_NAME).toBe("Resolved");
    expect(CREATED_SERIES_NAME).toBe("Created");
    expect(typeof headlineCardsForTab).toBe("function");
    expect(typeof selectHeadlineValue).toBe("function");
    expect(typeof selectAnalyticsTab).toBe("function");
    expect(typeof isAnalyticsTabId).toBe("function");
    expect(typeof analyticsTabHref).toBe("function");
    expect(typeof defaultAnalyticsTabHref).toBe("function");
    expect(typeof normalizeAnalyticsScope).toBe("function");
    expect(typeof selectAnalyticsProjects).toBe("function");
    expect(typeof resolveAnalyticsScope).toBe("function");
    expect(typeof parseAnalyticsSearch).toBe("function");
    expect(typeof analyticsScopeSearch).toBe("function");
    expect(typeof selectPickerSummary).toBe("function");
    expect(ANALYTICS_TAB_IDS).toContain("overview");
    expect(DEFAULT_ANALYTICS_TAB).toBe("overview");
    expect(ANALYTICS_TAB_LABELS["overview"]).toBe("Overview");
    expect(OVERVIEW_HEADLINE_CARDS.length).toBe(11);
    expect(WORK_ITEMS_HEADLINE_CARDS.length).toBe(5);
    expect(ALL_PROJECTS_LABEL).toBe("All projects");
    expect(MANY_PROJECTS_LABEL).toBe("3+ projects");
  });
});
