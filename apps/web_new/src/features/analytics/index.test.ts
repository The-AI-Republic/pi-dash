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
  analyticsKeys,
  analyticsProjectsChartQueryOptions,
  analyticsProjectStatsQueryOptions,
  analyticsScopeSearch,
  analyticsTabHref,
  analyticsTotalsQueryOptions,
  completionPercent,
  completionTone,
  DEFAULT_ANALYTICS_TAB,
  defaultAnalyticsTabHref,
  HeadlineCards,
  headlineCardsForTab,
  isAnalyticsTabId,
  MANY_PROJECTS_LABEL,
  normalizeAnalyticsScope,
  OVERVIEW_HEADLINE_CARDS,
  parseAnalyticsSearch,
  RadarSection,
  resolveActiveProjects,
  resolveAnalyticsScope,
  selectAnalyticsProjects,
  selectAnalyticsTab,
  selectHeadlineValue,
  selectPickerSummary,
  useAnalyticsProjectsChart,
  useAnalyticsProjectStats,
  useAnalyticsTotals,
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
    expect(typeof useAnalyticsProjectsChart).toBe("function");
    expect(typeof useAnalyticsProjectStats).toBe("function");
    expect(typeof RadarSection).toBe("function");
    expect(typeof ActiveProjectsSection).toBe("function");
    expect(typeof AnalyticsSectionGuidance).toBe("function");
    expect(typeof completionPercent).toBe("function");
    expect(typeof completionTone).toBe("function");
    expect(typeof resolveActiveProjects).toBe("function");
    expect(typeof analyticsKeys.totals).toBe("function");
    expect(typeof analyticsKeys.charts).toBe("function");
    expect(typeof analyticsKeys.projectStats).toBe("function");
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
