// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the home feature (NEWFRONT-278 top half; NEWFRONT-280
// recents plus dashboard chrome; NEWFRONT-279 quicklinks plus widget
// management). Other features and routes import only from here.

export { logHomeFetch } from "./api/fetch.js";
export { homeKeys } from "./api/keys.js";
export { previewRenameMutationOptions, usePreviewRename } from "./api/previewRename.js";
export { projectStatesKeys, projectStatesQueryOptions } from "./api/projectStates.js";
export {
  createQuickLinkMutationOptions,
  deleteQuickLinkMutationOptions,
  quicklinksKeys,
  quicklinksQueryOptions,
  updateQuickLinkMutationOptions,
  useCreateQuickLink,
  useDeleteQuickLink,
  useQuickLinks,
  useUpdateQuickLink,
} from "./api/quicklinks.js";
export type { QuickLinkDraft, QuickLinkEdit } from "./api/quicklinks.js";
export {
  homeAssistantConfigQueryOptions,
  homeThreadsQueryOptions,
  useAssistantConfig,
  useAssistantThreads,
} from "./api/queries.js";
export { recentsKeys, recentsQueryOptions, useRecentVisits } from "./api/recents.js";
export { widgetPrefsKeys, widgetPrefsQueryOptions, useWidgetPrefs } from "./api/widgetPrefs.js";
export {
  reorderWidgetMutationOptions,
  toggleWidgetMutationOptions,
  useReorderWidget,
  useToggleWidget,
} from "./api/widgetUpdates.js";
export type { WidgetReorderInput, WidgetToggle } from "./api/widgetUpdates.js";
export { isAssistantConfigured } from "../assistant/index.js";
export { HomeAllOffState } from "./components/HomeAllOffState.js";
export { HomeAssistantCard } from "./components/HomeAssistantCard.js";
export type { HomeAssistantCardProps } from "./components/HomeAssistantCard.js";
export { HomeDashboard } from "./components/HomeDashboard.js";
export type { HomeDashboardProps } from "./components/HomeDashboard.js";
export { HomeGreeting } from "./components/HomeGreeting.js";
export { QuickstartGuide } from "./components/QuickstartGuide.js";
export type { QuickstartGuideProps } from "./components/QuickstartGuide.js";
export { HomeWidgetStack } from "./components/HomeWidgetStack.js";
export type { HomeWidgetStackProps, ManageWidgetsControls } from "./components/HomeWidgetStack.js";
export { ManageWidgetsDialog } from "./components/ManageWidgetsDialog.js";
export type { ManageWidgetsDialogProps } from "./components/ManageWidgetsDialog.js";
export { QuickLinkDialog } from "./components/QuickLinkDialog.js";
export type { QuickLinkDialogProps } from "./components/QuickLinkDialog.js";
export { QuicklinksWidget } from "./components/QuicklinksWidget.js";
export { RecentIssueRow, RecentPageRow, RecentProjectRow } from "./components/RecentRows.js";
export type { RecentIssueRowProps, RecentPageRowProps, RecentProjectRowProps } from "./components/RecentRows.js";
export { RecentsWidget } from "./components/RecentsWidget.js";
export { priorityDisplayLabel, priorityTone } from "./lib/priority.js";
export {
  indexProjectIdentifiers,
  indexStateLists,
  issueDetailHref,
  issueKeyLabel,
  pageHref,
  parseRecentRow,
  parseRecentRows,
  projectHref,
  RECENT_FILTER_LABELS,
  RECENT_FILTERS,
  recentProjectIds,
  recentsEntityParam,
  resolveIssueState,
} from "./lib/recentRows.js";
export type {
  RecentEntityData,
  RecentFilter,
  RecentIssueData,
  RecentPageData,
  RecentProjectData,
  RecentRow,
  ResolvedState,
} from "./lib/recentRows.js";
export { formatRelativeVisitTime } from "./lib/relativeTime.js";
export { planWidgetReorder, surfacedWidgetPrefs, widgetDisplayName } from "./lib/manageWidgets.js";
export type { WidgetReorder } from "./lib/manageWidgets.js";
export { normalizeQuickLinkAddress, quickLinkDisplayName, validateQuickLinkForm } from "./lib/quicklinks.js";
export type { NormalizedQuickLink, QuickLinkForm, QuickLinkValidation } from "./lib/quicklinks.js";
export { orderWidgetPrefs, visibleWidgetPrefs, widgetSectionKind } from "./lib/widgets.js";
export type { WidgetSectionKind } from "./lib/widgets.js";
