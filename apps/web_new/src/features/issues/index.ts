// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the issues feature. Other features import only from here.

export { issueKeys } from "./api/keys.js";
export { issuesQueryOptions } from "./api/queries.js";
export {
  DEFAULT_LAYOUT,
  LAYOUT_LABELS,
  LAYOUT_ORDER,
  MOBILE_LAYOUT_ORDER,
  isLayoutKind,
  mobileOfferedLayouts,
  parseLayoutKind,
  resolveActiveLayout,
} from "./layouts/kinds.js";
export type { LayoutKind } from "./layouts/kinds.js";
export { IssuesLayoutRoot } from "./layouts/IssuesLayoutRoot.js";
export type { IssuesLayoutRootProps, LayoutRenderer } from "./layouts/IssuesLayoutRoot.js";
export { entityListsKeyPart, writeEntityPrefs } from "./prefs/entities.js";
export type { IssuesEntity, PrefsRecord } from "./prefs/entities.js";
export { entityPrefsKeyPart, prefsQueryOptions, readEntityPrefs } from "./prefs/prefsQuery.js";
export {
  buildPrefsPatch,
  parseDisplayPrefs,
  selectDisplayPrefs,
  useEntityPrefs,
  useUpdateDisplayPrefs,
} from "./prefs/displayPrefs.js";
export type { DisplayCalendarPrefs, DisplayPrefs } from "./prefs/displayPrefs.js";
export { IssuesLoaderProvider, useIssuesLoader } from "./loaders/loaderContext.js";
export type { IssuesLoader } from "./loaders/loaderContext.js";
export { LayoutBody } from "./loaders/LayoutBody.js";
export type { LayoutBodyProps } from "./loaders/LayoutBody.js";
export { MutationSpinner } from "./loaders/MutationSpinner.js";
export {
  CalendarReloadBar,
  CalendarSkeleton,
  GanttSkeleton,
  KanbanSkeleton,
  LayoutSkeleton,
  SpreadsheetSkeleton,
} from "./loaders/skeletons.js";
export { ListSkeleton } from "./loaders/ListSkeleton.js";
export {
  FRESH_WINDOW_MS,
  createTempId,
  isFresh,
  isRowHighlighted,
  isTempRowId,
  partitionFresh,
  rowDomId,
} from "./freshness/fresh.js";
export type { FreshPartition } from "./freshness/fresh.js";
export { HighlightProvider, useHighlight } from "./freshness/highlight.js";
export type { HighlightHandle, HighlightOptions } from "./freshness/highlight.js";
export { insertTempRow, removeTempRow, tempIssueRow } from "./freshness/tempRows.js";
export { LayoutIcon } from "./components/LayoutIcon.js";
export type { LayoutIconProps } from "./components/LayoutIcon.js";
export { layoutMenuItems } from "./components/layoutMenuItems.js";
export { LayoutSwitcher } from "./components/LayoutSwitcher.js";
export type { LayoutSwitcherProps } from "./components/LayoutSwitcher.js";
export { IssuesHeader } from "./components/IssuesHeader.js";
export type { IssuesHeaderProps } from "./components/IssuesHeader.js";
export { MobileIssuesHeader } from "./components/MobileIssuesHeader.js";
export type { MobileIssuesHeaderProps } from "./components/MobileIssuesHeader.js";
export { useCreateWorkItem, WORK_ITEM_CREATED_TITLE, WORK_ITEM_CREATE_FAILED_TITLE } from "./api/create.js";
export type { WorkItemCreateInput } from "./api/create.js";
export { clearWorkItemDraft, loadWorkItemDraft, storeWorkItemDraft, workItemDraftKey } from "./lib/createDraft.js";
export type { WorkItemDraft } from "./lib/createDraft.js";
export { QuickCreateButton } from "./components/QuickCreateButton.js";
export type { QuickCreateButtonProps } from "./components/QuickCreateButton.js";
export { WorkItemCreateDialog } from "./components/WorkItemCreateDialog.js";
export type { WorkItemCreateDialogProps, WorkItemCreateProject } from "./components/WorkItemCreateDialog.js";
export { browseIssueQueryOptions, loadBrowseIssue, matchBrowseRow } from "./api/browse.js";
export type { BrowseResolution, BrowseSearchRow } from "./api/browse.js";
export { BrowseDetail, BrowseUnknownKey } from "./components/BrowseDetail.js";
export type { BrowseDetailProps } from "./components/BrowseDetail.js";
export { IssueList } from "./components/IssueList.js";
export type { IssueListProps } from "./components/IssueList.js";
export { IssueLayout, IssueSearch, parseIssueSearch } from "./filters/search.js";
export {
  CAPTION_YEAR_MAX,
  CAPTION_YEAR_MIN,
  DEFAULT_WEEK_START,
  MONTH_ABBREVS,
  MONTH_NAMES,
  WEEKDAY_NAMES,
  WEEKDAY_SHORT,
  applyRangeClick,
  formatDay,
  formatDayLong,
  formatMergedRange,
  isDayInRange,
  isIsoDay,
  monthGrid,
  normalizeWeekStart,
  orderedWeekdays,
  parseIsoDay,
  shiftMonth,
  todayIsoDay,
  toIsoDay,
} from "./dropdowns/calendar.js";
export type { CalendarDay, RangeValue } from "./dropdowns/calendar.js";
export { DateCalendar, DatePicker } from "./dropdowns/DatePicker.js";
export type { DateCalendarProps, DatePickerProps } from "./dropdowns/DatePicker.js";
export { DateRangeCalendar, DateRangePicker } from "./dropdowns/DateRangePicker.js";
export type { DateRangeCalendarProps, DateRangeMode, DateRangePickerProps } from "./dropdowns/DateRangePicker.js";
export { useWeekStart } from "./dropdowns/weekStart.js";
export { ISSUE_DATES_FAILED_TITLE, useUpdateIssueDates, withIssueDates } from "./api/dates.js";
export type { IssueDatesInput } from "./api/dates.js";
