// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the analytics feature. Other features import only from here.

export { analyticsKeys } from "./api/keys.js";
export { analyticsTotalsQueryOptions, useAnalyticsTotals } from "./api/totals.js";
export { AnalyticsEmptyView } from "./components/AnalyticsEmptyView.js";
export type { AnalyticsEmptyViewProps } from "./components/AnalyticsEmptyView.js";
export { AnalyticsPicker } from "./components/AnalyticsPicker.js";
export type { AnalyticsPickerProject, AnalyticsPickerProps } from "./components/AnalyticsPicker.js";
export { AnalyticsScreen } from "./components/AnalyticsScreen.js";
export type { AnalyticsScreenProps } from "./components/AnalyticsScreen.js";
export { AnalyticsTabStrip } from "./components/AnalyticsTabStrip.js";
export type { AnalyticsTabStripProps } from "./components/AnalyticsTabStrip.js";
export { HeadlineCards } from "./components/HeadlineCards.js";
export type { HeadlineCardsProps } from "./components/HeadlineCards.js";
export {
  headlineCardsForTab,
  OVERVIEW_HEADLINE_CARDS,
  selectHeadlineValue,
  WORK_ITEMS_HEADLINE_CARDS,
} from "./lib/cards.js";
export type { HeadlineCardDef } from "./lib/cards.js";
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
