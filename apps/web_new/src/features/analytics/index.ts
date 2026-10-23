// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the analytics feature. Other features import only from here.

export { analyticsKeys } from "./api/keys.js";
export type { AnalyticsChartKind } from "./api/keys.js";
export {
  analyticsCustomChartQueryOptions,
  analyticsProjectsChartQueryOptions,
  analyticsWorkItemsChartQueryOptions,
  useAnalyticsCustomChart,
  useAnalyticsProjectsChart,
  useAnalyticsWorkItemsChart,
} from "./api/charts.js";
export type { AnalyticsCustomChartSelection } from "./api/charts.js";
export { analyticsProjectStatsQueryOptions, useAnalyticsProjectStats } from "./api/projectStats.js";
export { analyticsTotalsQueryOptions, useAnalyticsTotals } from "./api/totals.js";
export { ActiveProjectsSection } from "./components/ActiveProjectsSection.js";
export type { ActiveProjectsSectionProps } from "./components/ActiveProjectsSection.js";
export { AnalyticsEmptyView } from "./components/AnalyticsEmptyView.js";
export type { AnalyticsEmptyViewProps } from "./components/AnalyticsEmptyView.js";
export { AnalyticsPicker } from "./components/AnalyticsPicker.js";
export type { AnalyticsPickerProject, AnalyticsPickerProps } from "./components/AnalyticsPicker.js";
export { AnalyticsScreen } from "./components/AnalyticsScreen.js";
export type { AnalyticsScreenProps } from "./components/AnalyticsScreen.js";
export { AnalyticsTabStrip } from "./components/AnalyticsTabStrip.js";
export type { AnalyticsTabStripProps } from "./components/AnalyticsTabStrip.js";
export { CreatedResolvedSection } from "./components/CreatedResolvedSection.js";
export type { CreatedResolvedSectionProps } from "./components/CreatedResolvedSection.js";
export { CustomInsightPickers } from "./components/CustomInsightPickers.js";
export type { CustomInsightPickersProps } from "./components/CustomInsightPickers.js";
export { CustomInsightsSection } from "./components/CustomInsightsSection.js";
export type { CustomInsightsSectionProps } from "./components/CustomInsightsSection.js";
export { HeadlineCards } from "./components/HeadlineCards.js";
export type { HeadlineCardsProps } from "./components/HeadlineCards.js";
export { RadarSection } from "./components/RadarSection.js";
export type { RadarScopeProject, RadarSectionProps } from "./components/RadarSection.js";
export { AnalyticsSectionGuidance } from "./components/AnalyticsSectionGuidance.js";
export type { AnalyticsSectionGuidanceProps } from "./components/AnalyticsSectionGuidance.js";
export {
  headlineCardsForTab,
  OVERVIEW_HEADLINE_CARDS,
  selectHeadlineValue,
  WORK_ITEMS_HEADLINE_CARDS,
} from "./lib/cards.js";
export type { HeadlineCardDef } from "./lib/cards.js";
export { completionPercent, completionTone, resolveActiveProjects } from "./lib/activeProjects.js";
export type { ActiveProjectRow, ActiveProjectSource, CompletionTone } from "./lib/activeProjects.js";
export {
  CREATED_SERIES_KEY,
  CREATED_SERIES_NAME,
  formatSeriesMonth,
  RESOLVED_SERIES_KEY,
  RESOLVED_SERIES_NAME,
  selectCreatedResolvedColors,
  selectCreatedResolvedPoints,
} from "./lib/createdResolved.js";
export type { CreatedResolvedColors, CreatedResolvedPoint } from "./lib/createdResolved.js";
export {
  CUSTOM_AXES,
  CUSTOM_METRICS,
  CUSTOM_SPLIT_CLEAR_LABEL,
  CUSTOM_SPLIT_EMPTY_LABEL,
  customAxisLabel,
  customMetricLabel,
  DEFAULT_CUSTOM_METRIC,
  DEFAULT_CUSTOM_SPLIT,
  DEFAULT_CUSTOM_X_AXIS,
  isCapitalizedCustomAxis,
  isCustomDateAxis,
  selectCustomSplitOptions,
  selectCustomXOptions,
} from "./lib/customAxes.js";
export type { CustomAxisDef, CustomMetricDef, CustomSplitDef } from "./lib/customAxes.js";
export {
  buildCustomChartModel,
  capitalizeCustomLabel,
  CUSTOM_SINGLE_SERIES_NAME,
  formatCustomDateLabel,
  indexStateColors,
  selectCustomBarLabel,
  selectCustomSegmentLabel,
  selectPriorityBarColor,
  selectStateBarColor,
} from "./lib/customChart.js";
export type {
  CustomChartBar,
  CustomChartInput,
  CustomChartModel,
  CustomChartSegment,
  StateColorIndex,
} from "./lib/customChart.js";
export {
  ALL_PROJECTS_LABEL,
  analyticsScopeSearch,
  MANY_PROJECTS_LABEL,
  normalizeAnalyticsScope,
  parseAnalyticsSearch,
  resolveAnalyticsScope,
  selectAnalyticsProjects,
  selectPickerSummary,
} from "./lib/scope.js";
export type { AnalyticsSearch, ScopeProject } from "./lib/scope.js";
export {
  ANALYTICS_TAB_IDS,
  ANALYTICS_TAB_LABELS,
  analyticsTabHref,
  DEFAULT_ANALYTICS_TAB,
  defaultAnalyticsTabHref,
  isAnalyticsTabId,
  selectAnalyticsTab,
} from "./lib/tabs.js";
export type { AnalyticsTabId } from "./lib/tabs.js";
