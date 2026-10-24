// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the issues feature. Other features import only from here.

export { issueKeys } from "./api/keys.js";
export { issuesQueryOptions } from "./api/queries.js";
export { listPageQueryOptions } from "./list/pages.js";
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
export { ViewLayoutBody, preloadViewLayout } from "./layouts/ViewLayoutBody.js";
export type { ViewLayoutBodyProps } from "./layouts/ViewLayoutBody.js";
export { SavedViewEmptyCard, loadSavedViewEmptyCard } from "./emptyStates/viewEmpty.js";
export type { SavedViewEmptyCardProps } from "./emptyStates/viewEmpty.js";
export { entityListsKeyPart, writeEntityPrefs } from "./prefs/entities.js";
export type { IssuesEntity, PrefsRecord } from "./prefs/entities.js";
export {
  entityPrefsKeyPart,
  orderByFromDisplayFilters,
  prefsQueryOptions,
  readEntityPrefs,
} from "./prefs/prefsQuery.js";
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
export { ListPageQuickActions } from "./menus/ListPageQuickActions.js";
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
export { useAttachDescriptionAssets } from "./create/createApi.js";
export { ModalDescription } from "./create/ModalDescription.js";
export type { ModalDescriptionProps } from "./create/ModalDescription.js";
export { DEDUPE_DEBOUNCE_MS, DuplicatePanel, NO_MATCHES, useDuplicateMatches } from "./create/dedupe.js";
export type { DuplicateMatches, DuplicateSearchInput } from "./create/dedupe.js";
export type { WorkItemCreateDialogProps, WorkItemCreateProject } from "./components/WorkItemCreateDialog.js";
export type { WorkItemEditTarget } from "./components/WorkItemCreateDialog.js";
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
export { ACTIVITY_HEADING, FeedHeader } from "./activity/FeedHeader.js";
export type { FeedHeaderProps } from "./activity/FeedHeader.js";
export { ActivityEntryRow } from "./activity/ActivityEntryRow.js";
export type { ActivityEntryRowProps } from "./activity/ActivityEntryRow.js";
export { ActivityFeed, FEED_LOAD_FAILED, FeedSkeleton } from "./activity/ActivityFeed.js";
export type { ActivityFeedProps } from "./activity/ActivityFeed.js";
export { activitiesQueryOptions, useIssueActivities } from "./activity/api.js";
export type { ActivityScope } from "./activity/api.js";
export {
  DEFAULT_FILTERS,
  DEFAULT_SORT,
  FEED_FILTERS,
  FEED_FILTER_LABELS,
  FILTERS_STORAGE_KEY,
  SORT_STORAGE_KEY,
  activityActorName,
  activityFilterOf,
  activitySentence,
  buildFeed,
  composeFeed,
  feedEntryVisible,
  filterFeed,
  flipSort,
  isFilterNarrowed,
  parseStoredFilters,
  parseStoredSort,
  serializeFilters,
  serializeSort,
  sortFeed,
  toggleFeedFilter,
} from "./activity/model.js";
export type { FeedEntry, FeedFilter, FeedSortOrder } from "./activity/model.js";
export { useFeedPrefs } from "./activity/prefs.js";
export type { FeedPrefs } from "./activity/prefs.js";
export { EMOJI_PICKER_LABEL, EmojiPicker } from "./reactions/EmojiPicker.js";
export type { EmojiPickerProps } from "./reactions/EmojiPicker.js";
export { REACTED_CHIP_CLASS, ReactionBar } from "./reactions/ReactionBar.js";
export type { ReactionBarProps } from "./reactions/ReactionBar.js";
export { ReactionPicker } from "./reactions/ReactionPicker.js";
export type { ReactionPickerProps } from "./reactions/ReactionPicker.js";
export { COMMENT_REACTIONS_LABEL, CommentReactionPicker, CommentReactions } from "./reactions/CommentReactions.js";
export type { CommentReactionsProps } from "./reactions/CommentReactions.js";
export { ISSUE_REACTIONS_LABEL, IssueReactions } from "./reactions/IssueReactions.js";
export type { IssueReactionsProps } from "./reactions/IssueReactions.js";
export {
  REACTION_CHOICES,
  chipTooltip,
  commentReactionRows,
  emojiToReaction,
  groupReactions,
  issueReactionRows,
  reactionAction,
  reactionToEmoji,
  viewerHasReaction,
} from "./reactions/model.js";
export type { ReactionChip, ReactionRow } from "./reactions/model.js";
export {
  REACTION_CREATED_TITLE,
  REACTION_REMOVED_TITLE,
  commentReactionsQueryOptions,
  issueReactionsQueryOptions,
  reactionFailureDetail,
  useCommentReactions,
  useIssueReactions,
  useToggleCommentReaction,
  useToggleIssueReaction,
} from "./reactions/api.js";
export type { ReactionScope } from "./reactions/api.js";

export type { LiteResolution } from "./api/browse.js";
export { issueDetailQueryOptions } from "./api/queries.js";
export { descriptionVersionQueryOptions, descriptionVersionsQueryOptions } from "./api/descriptionVersions.js";
export {
  DESCRIPTION_SAVE_FAILED_TITLE,
  TITLE_SAVE_FAILED_TITLE,
  saveFailureDetail,
  useUpdateIssue,
} from "./api/mutations.js";
export type { UpdateIssueScope } from "./api/mutations.js";
export { DetailMainColumn } from "./detail/DetailMainColumn.js";
export type { DetailFieldWrites, DetailMainColumnProps } from "./detail/DetailMainColumn.js";
export { DetailMissingState } from "./detail/DetailMissingState.js";
export type { DetailMissingStateProps } from "./detail/DetailMissingState.js";
export { DetailSkeleton } from "./detail/DetailSkeleton.js";
export { DescriptionHistory } from "./detail/DescriptionHistory.js";
export type { DescriptionHistoryProps, VersionAuthor } from "./detail/DescriptionHistory.js";
export { IssueDescriptionField } from "./detail/IssueDescriptionField.js";
export type { IssueDescriptionFieldProps, RestoredDescription } from "./detail/IssueDescriptionField.js";
export { IssueDetailRoot } from "./detail/IssueDetailRoot.js";
export { IssueAgentActionsEntry } from "./detail/lazyAgentActions.js";
export type { IssueAgentActionsEntryProps } from "./detail/lazyAgentActions.js";
export {
  PARENT_BANNER_LABEL,
  ParentBanner,
  REMOVE_PARENT_TEXT,
  SIBLINGS_EMPTY_TEXT,
  SIBLINGS_LOADING_TEXT,
} from "./detail/ParentBanner.js";
export type { ParentBannerProps } from "./detail/ParentBanner.js";
export {
  PARENT_SAVE_FAILED_TITLE,
  parentIssueQueryOptions,
  parentSiblingsQueryOptions,
  siblingRows,
  useParentWrite,
} from "./api/parent.js";
export type { ParentWriteScope } from "./api/parent.js";
export { IssuePropertiesSidebar } from "./detail/sidebar/index.js";
export type { IssuePropertiesSidebarProps } from "./detail/sidebar/index.js";
export {
  SIDEBAR_ROW_CLASS,
  SIDEBAR_ROW_LABEL_CLASS,
  SidebarRow,
  dateBounds,
  isDueDateOverdue,
} from "./detail/sidebar/index.js";
export type { SidebarRowProps } from "./detail/sidebar/index.js";
export { AttachmentsWidget } from "./detail/widgets/attachments/AttachmentsWidget.js";
export type { AttachmentsWidgetProps } from "./detail/widgets/attachments/AttachmentsWidget.js";
export type { IssueDetailRootProps } from "./detail/IssueDetailRoot.js";
export { TriageDetailRoot } from "./detail/TriageDetailRoot.js";
export type { TriageDetailRootProps } from "./detail/TriageDetailRoot.js";
export { IssueTitleField, TITLE_AUTOSAVE_MS, TITLE_MAX_LENGTH } from "./detail/IssueTitleField.js";
export type { IssueTitleFieldProps } from "./detail/IssueTitleField.js";
export { AgentStatusPanel } from "./detail/agent/AgentStatusPanel.js";
export type { AgentStatusPanelProps } from "./detail/agent/AgentStatusPanel.js";
export { DetailHeaderActions } from "./detail/DetailHeaderActions.js";
export type { DetailHeaderActionsProps } from "./detail/DetailHeaderActions.js";
export { IssueIdentifier, issueKeyOf } from "./detail/IssueIdentifier.js";
export type { IssueIdentifierProps } from "./detail/IssueIdentifier.js";
export { IssueSubscribeToggle } from "./detail/IssueSubscribeToggle.js";
export type { IssueSubscribeToggleProps } from "./detail/IssueSubscribeToggle.js";
export { SaveIndicator } from "./detail/SaveIndicator.js";
export { useSavePipeline } from "./detail/savePipeline.js";
export type { SavePipeline, SaveStatus } from "./detail/savePipeline.js";
export { useDetailUiStore } from "./detail/store.js";
export { IssuePeek, keepPeekOnClick, peekIdentifierOf, peekKeyOf, peekLinkKeyOf } from "./peek/IssuePeek.js";
export type { IssuePeekProps, PeekKey } from "./peek/IssuePeek.js";
export { ResolvingIssuePeek } from "./peek/ResolvingIssuePeek.js";
export type { ResolvingIssuePeekProps } from "./peek/ResolvingIssuePeek.js";
export { PeekChrome } from "./peek/PeekChrome.js";
export type { PeekChromeProps } from "./peek/PeekChrome.js";
export { PeekModeToggle, peekModeLabel, PEEK_MODES } from "./peek/PeekModeToggle.js";
export type { PeekMode, PeekModeToggleProps } from "./peek/PeekModeToggle.js";
export {
  PEEK_LOADING_LABEL,
  PEEK_MISSING_BODY,
  PEEK_MISSING_TITLE,
  PeekErrorPanel,
  PeekLoadingPanel,
} from "./peek/peekStates.js";
export type { PeekStatusPanelProps } from "./peek/peekStates.js";
export { isIssueKey, resolvePeekTarget } from "./peek/peekTarget.js";
export type { PeekRows, PeekTarget } from "./peek/peekTarget.js";
export {
  nestingSearchFor,
  parseNestingLevel,
  PEEK_ISSUE_PARAM,
  PEEK_NESTING_PARAM,
  PEEK_PROJECT_PARAM,
  stripPeekParams,
} from "./peek/peekUrl.js";
export { CARD_COPY_COPIED, CARD_COPY_FAILED, copyCardLink } from "./peek/peekLink.js";
export { IssueList } from "./components/IssueList.js";
export type { IssueListProps } from "./components/IssueList.js";
export { CalendarLayout } from "./calendar/CalendarLayout.js";
export type { CalendarLayoutProps } from "./calendar/CalendarLayout.js";
export { CalendarHeader } from "./calendar/CalendarHeader.js";
export type { CalendarHeaderProps, CalendarOptionsValue } from "./calendar/CalendarHeader.js";
export { CalendarGrid } from "./calendar/CalendarGrid.js";
export type { CalendarGridProps } from "./calendar/CalendarGrid.js";
export { CalendarDay as CalendarTile } from "./calendar/CalendarDay.js";
export type { CalendarDayProps as CalendarTileProps } from "./calendar/CalendarDay.js";
export { CalendarBlock } from "./calendar/CalendarBlock.js";
export type { CalendarBlockProps } from "./calendar/CalendarBlock.js";
export {
  calendarDayQueryOptions,
  calendarQueryOptions,
  calendarStatesQueryOptions,
  rowsFromResponse,
} from "./calendar/query.js";
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
  orderedWeekdays,
  parseIsoDay,
  shiftMonth,
  todayIsoDay,
  toIsoDay,
} from "./dropdowns/calendar.js";
export { DEFAULT_WEEK_START, normalizeWeekStart } from "./dropdowns/weekDays.js";
export type { CalendarDay, RangeValue } from "./dropdowns/calendar.js";
export { DateCalendar, DatePicker } from "./dropdowns/DatePicker.js";
export type { DateCalendarProps, DatePickerProps } from "./dropdowns/DatePicker.js";
export { DateRangeCalendar, DateRangePicker } from "./dropdowns/DateRangePicker.js";
export type { DateRangeCalendarProps, DateRangeMode, DateRangePickerProps } from "./dropdowns/DateRangePicker.js";
export { useWeekStart } from "./dropdowns/weekStart.js";
export { FilterSection } from "./filters/shared/FilterSection.js";
export type { FilterSectionOption, FilterSectionProps } from "./filters/shared/FilterSection.js";
export {
  DATE_PRESETS,
  customDateMode,
  customEndEntry,
  customRange,
  customStartEntry,
  hasCustomDate,
  isDatePreset,
} from "./filters/rich/legacyDimensions.js";
export type { CustomDateMode, DatePreset } from "./filters/rich/legacyDimensions.js";
// Saved-view detail seam (VIEW-025/026, NEWFRONT-724). The views area hosts
// the row through the lazy boundary, never a static edge into rich/: the
// loader module carries only the dynamic import, so the filter graph stays
// out of eager closures. The parts type is compile-time only.
export { LazyIssueFilters, loadIssueFilters, preloadIssueFilters } from "./filters/rich/lazy.js";
export type { RichFilterBuilderParts } from "./filters/rich/RichFilterBuilder.js";
// Shared filter model seam (CYC-039, NEWFRONT-713). The cycle sidebar
// toggles breakdown entries through the same condition codec and slot
// helpers the layouts use; the codec already rides the initial bundle
// behind the layouts, so these named edges add no new chunk.
export { useRichFilters, useUpdateRichFilters } from "./filters/rich/useRichFilters.js";
export { conditionKey, findField, richFilterFields } from "./filters/rich/expression.js";
export { selectedIds, togglePick } from "./filters/rich/slotModel.js";
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
export type { GanttTimelineEntity } from "./gantt/GanttSidebar.js";
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
export { GanttQuickAdd } from "./gantt/GanttQuickAdd.js";
export type { GanttQuickAddProps } from "./gantt/GanttQuickAdd.js";
export { canEditTimeline } from "./gantt/permissions.js";
export type { TimelineStanding } from "./gantt/permissions.js";
export {
  DRAG_THRESHOLD_PX,
  RANK_STEP,
  baseWindow,
  dayIndexAt,
  deriveReorder,
  handlePreviewDate,
  movedDates,
  plantedDates,
  resizedDates,
  snappedDayDelta,
  windowDays,
} from "./gantt/drag.js";
export type { DropPlacement, GestureDates, ReorderEntry, ReorderResult } from "./gantt/drag.js";
export {
  GANTT_DATES_FAILED_TITLE,
  GANTT_REORDER_FAILED_TITLE,
  useGanttMutations,
  withIssueOrder,
} from "./gantt/useGanttMutations.js";
export type { GanttMutations } from "./gantt/useGanttMutations.js";
export { AnalyticsEntry, groupCounts } from "./analytics/AnalyticsEntry.js";
export type { AnalyticsEntryProps } from "./analytics/AnalyticsEntry.js";
export type { SpreadsheetLayoutProps } from "./spreadsheet/SpreadsheetLayout.js";
export {
  loadIssueListLeaves,
  primeIssueListLeaves,
  ResolvedAnalyticsEntry,
  ResolvedIssueList,
} from "./components/lazyLeaves.js";
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
export { SubIssuesWidget, WIDGET_ACTION_LABELS } from "./subIssues/SubIssuesWidget.js";
export type { WidgetActionLabel, WidgetSectionSlot, SubIssuesWidgetProps } from "./subIssues/SubIssuesWidget.js";
export { SubIssueSection } from "./subIssues/SubIssueSection.js";
export type { SubIssueSectionHandle, SubIssueSectionProps } from "./subIssues/SubIssueSection.js";
export type { CreateParentPreset } from "./components/WorkItemCreateDialog.js";
// Saved-view dialog seam (VIEW-013, NEWFRONT-721; edit prefill/save,
// NEWFRONT-722; VIEW-035, NEWFRONT-726). The views area builds its
// create/edit dialog on the rich filter builder, the expression codec,
// the temporary registry and the snapshot helpers; it never reaches
// past this barrel into the rich/ modules. All eight arrive through the
// loader below: the builder chunk is lazy, and a static barrel edge to
// views.js makes the bundler hoist the shared expression codec into the
// initial bundle (size gate). The loader lives here, not in an eager
// module, so its dynamic imports stay lazy with its callers.
export type RichFilterBuilderModule = {
  useRichFilterBuilder: (typeof import("./filters/rich/RichFilterBuilder.js"))["useRichFilterBuilder"];
  serializeRichExpression: (typeof import("./filters/rich/RichFilterBuilder.js"))["serializeRichExpression"];
  parseRichExpression: (typeof import("./filters/rich/expression.js"))["parseRichExpression"];
  FieldConfigRegistry: (typeof import("./filters/rich/temporary.js"))["FieldConfigRegistry"];
  buildViewCreate: (typeof import("./filters/rich/views.js"))["buildViewCreate"];
  buildViewPatch: (typeof import("./filters/rich/views.js"))["buildViewPatch"];
  viewDisplaySnapshot: (typeof import("./filters/rich/views.js"))["viewDisplaySnapshot"];
  savedViewKeys: (typeof import("./filters/rich/views.js"))["savedViewKeys"];
};

let richFilterBuilderRequest: Promise<RichFilterBuilderModule> | null = null;

/** Start (or reuse) the builder-module request for lazy dialog hosts. */
export function loadRichFilterBuilder(): Promise<RichFilterBuilderModule> {
  richFilterBuilderRequest ??= Promise.all([
    import("./filters/rich/RichFilterBuilder.js"),
    import("./filters/rich/expression.js"),
    import("./filters/rich/views.js"),
    import("./filters/rich/temporary.js"),
  ]).then(([builder, expression, views, temporary]) => ({
    useRichFilterBuilder: builder.useRichFilterBuilder,
    serializeRichExpression: builder.serializeRichExpression,
    parseRichExpression: expression.parseRichExpression,
    FieldConfigRegistry: temporary.FieldConfigRegistry,
    buildViewCreate: views.buildViewCreate,
    buildViewPatch: views.buildViewPatch,
    viewDisplaySnapshot: views.viewDisplaySnapshot,
    savedViewKeys: views.savedViewKeys,
  }));
  return richFilterBuilderRequest;
}

/** Kick the request while the host list's data is still loading. */
export function preloadRichFilterBuilder(): void {
  void loadRichFilterBuilder().catch(() => {
    // The host's filter section surfaces the failure when it renders.
  });
}

export type { FieldConfigRegistry } from "./filters/rich/temporary.js";
export type { RichCondition, RichExpression } from "./filters/rich/expression.js";
export type { ViewSnapshot } from "./filters/rich/views.js";

// Workspace view detail seam (VIEW-038/040, NEWFRONT-729). The views area
// embeds the workspace issue list scoped to the open view and mounts the
// lazy filter row beneath the view header. The query helper stays eager:
// it only adds the date bridge over the already-eager legacy dimensions
// (the expression import is type-only), so the size gate is unaffected.
export { withRichFilters } from "./filters/rich/query.js";
export { GlobalIssuesScreen } from "./global/GlobalIssuesScreen.js";
export type { GlobalIssuesScreenProps } from "./global/GlobalIssuesScreen.js";
export { GLOBAL_ISSUES_PAGE_SIZE, globalIssuesQueryOptions } from "./global/queries.js";
export type { IssueFiltersProps } from "./filters/rich/IssueFilters.js";
export { EntityListScreen } from "./list/EntityListScreen.js";
export type { EntityListScreenProps } from "./list/EntityListScreen.js";
