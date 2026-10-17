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
export { liteIssueQueryOptions, loadLiteIssue, parseBrowseKey, resolveShortLink } from "./api/browse.js";
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
