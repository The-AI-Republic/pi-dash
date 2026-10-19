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
  mergeDisplayPrefs,
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
export { CREATE_WORK_ITEM_COMMAND_ID, WorkItemCreateEntry } from "./components/WorkItemCreateEntry.js";
export type { WorkItemCreateEntryProps } from "./components/WorkItemCreateEntry.js";
export { selectCreatableProjects, TITLE_MAX_LENGTH as CREATE_TITLE_MAX_LENGTH } from "./create/createForm.js";
export type { CreateFormState, CreateProjectOption } from "./create/createForm.js";
export type { WorkItemCreateDialogProps, WorkItemCreateProject } from "./components/WorkItemCreateDialog.js";
export { liteIssueQueryOptions, loadLiteIssue, parseBrowseKey, resolveShortLink } from "./api/browse.js";
export { CommentCard, commentDomId, commentMenuItems } from "./comments/CommentCard.js";
export type { CommentAccessFlags, CommentCardProps } from "./comments/CommentCard.js";
export { CommentComposer } from "./comments/CommentComposer.js";
export type { CommentComposerProps } from "./comments/CommentComposer.js";
export { CommentEditForm } from "./comments/CommentEditForm.js";
export type { CommentEditFormProps } from "./comments/CommentEditForm.js";
export { CommentThread, commentIdFromHash, commentLinkFor } from "./comments/CommentThread.js";
export type { CommentLocation, CommentThreadProps } from "./comments/CommentThread.js";
export {
  commentsQueryOptions,
  foldPatch,
  accessPatch,
  useCreateComment,
  useDeleteComment,
  useIssueComments,
  useUpdateComment,
  useUploadCommentImage,
} from "./comments/api.js";
export type { CommentScope, ComposerImage, CreateCommentInput } from "./comments/api.js";
export {
  commentAuthorAvatar,
  commentAuthorName,
  commentDeepLink,
  commentMenuEntries,
  isCommentAuthor,
  isCommentBodyEmpty,
  isCommentFolded,
  toggledAccess,
  withFold,
} from "./comments/model.js";
export type { CommentMenuEntry, CommentMenuFlags } from "./comments/model.js";

export type { LiteResolution } from "./api/browse.js";
export {
  descriptionVersionQueryOptions,
  descriptionVersionsQueryOptions,
  issueDetailQueryOptions,
} from "./api/queries.js";
export { DESCRIPTION_SAVE_FAILED_TITLE, TITLE_SAVE_FAILED_TITLE, useUpdateIssue } from "./api/mutations.js";
export type { UpdateIssueScope } from "./api/mutations.js";
export { DetailMainColumn } from "./detail/DetailMainColumn.js";
export type { DetailMainColumnProps } from "./detail/DetailMainColumn.js";
export { DetailMissingState } from "./detail/DetailMissingState.js";
export type { DetailMissingStateProps } from "./detail/DetailMissingState.js";
export { DetailSkeleton } from "./detail/DetailSkeleton.js";
export { DescriptionHistory } from "./detail/DescriptionHistory.js";
export type { DescriptionHistoryProps, VersionAuthor } from "./detail/DescriptionHistory.js";
export { IssueDescriptionField } from "./detail/IssueDescriptionField.js";
export type { IssueDescriptionFieldProps, RestoredDescription } from "./detail/IssueDescriptionField.js";
export { IssueDetailRoot } from "./detail/IssueDetailRoot.js";
export type { IssueDetailRootProps } from "./detail/IssueDetailRoot.js";
export { TriageDetailRoot } from "./detail/TriageDetailRoot.js";
export type { TriageDetailRootProps } from "./detail/TriageDetailRoot.js";
export { IssueTitleField, TITLE_AUTOSAVE_MS, TITLE_MAX_LENGTH } from "./detail/IssueTitleField.js";
export type { IssueTitleFieldProps } from "./detail/IssueTitleField.js";
export { SaveIndicator } from "./detail/SaveIndicator.js";
export { useSavePipeline } from "./detail/savePipeline.js";
export type { SavePipeline, SaveStatus } from "./detail/savePipeline.js";
export { useDetailUiStore } from "./detail/store.js";
export { IssueList } from "./components/IssueList.js";
export type { IssueListProps } from "./components/IssueList.js";
export { IssueLayout, IssueSearch, parseIssueSearch } from "./filters/search.js";
export {
  labelKeys,
  labelsQueryOptions,
  useCreateLabel,
  useDeleteLabel,
  useLabels,
  useMoveLabel,
  useUpdateLabel,
} from "./labels/api.js";
export type { LabelMoveInput, LabelUpdateInput } from "./labels/api.js";
export { LabelsSettings } from "./labels/LabelsSettings.js";
export type { LabelsSettingsProps } from "./labels/LabelsSettings.js";
export { LabelPicker } from "./labels/LabelPicker.js";
export type { LabelPickerProps } from "./labels/LabelPicker.js";
export { LabelChips } from "./labels/LabelChips.js";
export type { LabelChipsProps } from "./labels/LabelChips.js";
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
export {
  DISPLAY_SECTION_ORDER,
  DISPLAY_SECTION_TITLES,
  DISPLAY_SWITCH_LABELS,
  EPIC_SUB_ISSUE_COUNT_LABEL,
  GROUP_DIMENSION_LABELS,
  GROUP_NONE_LABEL,
  ORDER_OPTION_LABELS,
  PROPERTY_PILL_LABELS,
  groupDimensionLabel,
  isPropertyShown,
  layoutDisplayMatrix,
  propertyPillLabel,
  visibleGroupDimensions,
  visiblePropertyKeys,
} from "./display/matrix.js";
export type { DisplayFeatureFlags, DisplaySection, DisplaySwitchKey, LayoutDisplayMatrix } from "./display/matrix.js";
export { DisplayPanel } from "./display/DisplayPanel.js";
export type { DisplayPanelProps, DisplayPanelState } from "./display/DisplayPanel.js";
export { DisplayControl } from "./display/DisplayControl.js";
export type { DisplayControlProps } from "./display/DisplayControl.js";
export { LayoutPicker } from "./dropdowns/LayoutPicker.js";
export type { LayoutPickerProps } from "./dropdowns/LayoutPicker.js";
export { layoutOptionList } from "./dropdowns/layoutOptions.js";
export type { LayoutOption } from "./dropdowns/layoutOptions.js";
export { ESTIMATE_POINT_FAILED_TITLE, useUpdateEstimatePoint } from "./api/estimates.js";
export { EXECUTION_TARGET_FAILED_TITLE, useUpdateExecutionTarget } from "./api/executionTarget.js";
export { withIssueFields } from "./api/optimisticRows.js";
export { GanttLayout, barColorsFor, orderedRows } from "./gantt/GanttLayout.js";
export type { GanttLayoutProps } from "./gantt/GanttLayout.js";
export { GanttChart, SIDEBAR_WIDTH } from "./gantt/GanttChart.js";
export type { GanttChartProps } from "./gantt/GanttChart.js";
export { GanttHeader } from "./gantt/GanttHeader.js";
export type { GanttHeaderProps } from "./gantt/GanttHeader.js";
export { ROW_HEIGHT } from "./gantt/rows.js";
export type { GanttBarBox, GanttRow } from "./gantt/rows.js";
export {
  DAY_WIDTHS,
  GANTT_ZOOMS,
  GANTT_ZOOM_LABELS,
  addDays,
  centeredScroll,
  dayOffset,
  diffDays,
  headerTiers,
  initialRange,
  todaySegment,
} from "./gantt/scale.js";
export type { GanttDay, GanttRange, GanttSegment, GanttZoom } from "./gantt/scale.js";
export { useGanttViewport, clampScroll, FALLBACK_VIEWPORT_WIDTH } from "./gantt/useGanttViewport.js";
export type { GanttViewport } from "./gantt/useGanttViewport.js";
export { AnalyticsEntry, groupCounts } from "./analytics/AnalyticsEntry.js";
export type { AnalyticsEntryProps } from "./analytics/AnalyticsEntry.js";
export type { SpreadsheetLayoutProps } from "./spreadsheet/SpreadsheetLayout.js";
export {
  SHEET_COLUMNS,
  SHEET_DEFAULT_ORDER,
  SHEET_SORT_ASCENDING_LABEL,
  SHEET_SORT_CLEAR_LABEL,
  SHEET_SORT_DESCENDING_LABEL,
  SHEET_SORT_STORAGE_KEY,
  activeSheetSort,
  isSheetColumnKey,
  moveSheetFocus,
  parseStoredSheetSort,
  resolveSheetSort,
  serializeSheetSort,
  sheetCellId,
  sheetFirstCellId,
  sheetSortOrder,
  sheetSortState,
  visibleSheetColumns,
} from "./spreadsheet/model.js";
export type { SheetCell, SheetColumn, SheetSort, SheetSortDirection, SheetSortState } from "./spreadsheet/model.js";
export {
  SHEET_PAGE_SIZE,
  flattenSheetPages,
  sheetFailureDetail,
  sheetIssuesQueryOptions,
  sheetTempIssueRow,
  sheetTotalCount,
  useSheetCreateIssue,
  useSheetCycleUpdate,
  useSheetFieldUpdate,
  useSheetModuleUpdate,
  withSheetIssueFields,
  withSheetPages,
} from "./spreadsheet/data.js";
export { isEditableColumn } from "./spreadsheet/cells.js";
export { SHEET_SUB_ISSUE_LIMIT, sheetSubIssuesQueryOptions } from "./spreadsheet/subIssues.js";
