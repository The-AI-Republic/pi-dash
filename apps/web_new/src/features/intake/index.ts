// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the intake feature (parity/specs/intake.md). Routes import
// from here only; sibling slices (list rows, create, detail, actions) add
// their own exports as they land.

export { useCreateIntakeRequest } from "./api/create.js";
export { INTAKE_DEFAULT_ORDER, intakeKeys } from "./api/keys.js";
export type { IntakeListFilters, IntakeListKeyInput } from "./api/keys.js";
export {
  intakeDetailQueryOptions,
  intakeDetailWorkItem,
  isMissingIntakeDetail,
  useIntakeDetail,
} from "./api/detail.js";
export { paintIntakeDetailPatch, useUpdateIntakeIssue } from "./api/mutations.js";
export type { UpdateIntakeIssueScope } from "./api/mutations.js";
export {
  ACCEPT_FAILED_TITLE,
  DECLINE_FAILED_TITLE,
  DECLINE_SUCCESS_TITLE,
  DELETE_FAILED_TITLE,
  DELETE_SUCCESS_TITLE,
  DUPLICATE_FAILED_TITLE,
  DUPLICATE_SUCCESS_TITLE,
  SNOOZE_FAILED_TITLE,
  SNOOZE_SUCCESS_TITLE,
  UNSNOOZE_FAILED_TITLE,
  UNSNOOZE_SUCCESS_TITLE,
  duplicateTargetSearchQueryOptions,
  excludeCurrentTarget,
  invalidateTriageCaches,
  useDeleteIntakeRequest,
  useDuplicateTargetSearch,
  useTriageControls,
  useTriageStatusMove,
} from "./api/triage.js";
export type {
  DeleteIntakeRequestScope,
  TriageActionName,
  TriageControls,
  TriageControlsScope,
  TriageDialogName,
  TriageMoveScope,
  TriageStatusInput,
} from "./api/triage.js";
export {
  INTAKE_PAGE_SIZE,
  INTAKE_QUERY_POLICY,
  intakeListQueryOptions,
  intakeRows,
  intakeTotal,
  listStatuses,
  prefetchIntakeList,
  useIntakeList,
} from "./api/queries.js";
export type { IntakeListRead } from "./api/queries.js";
export { DECLINE_DIALOG_TITLE, IntakeDeclineDialog } from "./components/IntakeDeclineDialog.js";
export type { IntakeDeclineDialogProps } from "./components/IntakeDeclineDialog.js";
export {
  DELETE_ACCEPTED_DIALOG_TITLE,
  DELETE_DIALOG_TITLE,
  IntakeDeleteDialog,
} from "./components/IntakeDeleteDialog.js";
export type { IntakeDeleteDialogProps } from "./components/IntakeDeleteDialog.js";
export { IntakeDetail } from "./components/IntakeDetail.js";
export type { IntakeDetailProps } from "./components/IntakeDetail.js";
export { IntakeDetailHeader } from "./components/IntakeDetailHeader.js";
export type { IntakeDetailHeaderProps } from "./components/IntakeDetailHeader.js";
export {
  INTAKE_CREATED_BODY,
  INTAKE_CREATED_TITLE,
  INTAKE_CREATE_CANCEL_LABEL,
  INTAKE_CREATE_DIALOG_TITLE,
  INTAKE_CREATE_FAILED_BODY,
  INTAKE_CREATE_FAILED_TITLE,
  INTAKE_CREATE_MORE_LABEL,
  INTAKE_CREATE_SAVING_LABEL,
  INTAKE_CREATE_SUBMIT_LABEL,
  INTAKE_EDITOR_BUSY_BODY,
  INTAKE_EDITOR_BUSY_TITLE,
  IntakeCreateDialog,
} from "./components/IntakeCreateDialog.js";
export type { IntakeCreateDialogProps } from "./components/IntakeCreateDialog.js";
export {
  DUPLICATE_DIALOG_TITLE,
  DUPLICATE_SEARCH_DEBOUNCE_MS,
  DUPLICATE_SEARCH_LABEL,
  IntakeDuplicateDialog,
} from "./components/IntakeDuplicateDialog.js";
export type { IntakeDuplicateDialogProps } from "./components/IntakeDuplicateDialog.js";
export { IntakeEmptyState } from "./components/IntakeEmptyState.js";
export type { IntakeEmptyStateProps } from "./components/IntakeEmptyState.js";
export { IntakeProperties } from "./components/IntakeProperties.js";
export type { IntakePropertiesProps } from "./components/IntakeProperties.js";
export { SNOOZE_DIALOG_TITLE, IntakeSnoozeDialog } from "./components/IntakeSnoozeDialog.js";
export type { IntakeSnoozeDialogProps } from "./components/IntakeSnoozeDialog.js";
export { IntakeStatusChip } from "./components/IntakeStatusChip.js";
export type { IntakeStatusChipProps } from "./components/IntakeStatusChip.js";
export {
  ACCEPT_SAVE_LABEL,
  TRIAGE_ACCEPT_LABEL,
  TRIAGE_DECLINE_LABEL,
  IntakeTriageActions,
} from "./components/IntakeTriageActions.js";
export type { IntakeTriageActionsProps } from "./components/IntakeTriageActions.js";
export { IntakeGate } from "./components/IntakeGate.js";
export type { IntakeGateProps } from "./components/IntakeGate.js";
export { IntakeScreen } from "./components/IntakeScreen.js";
export type { IntakeScreenProps } from "./components/IntakeScreen.js";
export { INTAKE_LIST_PANEL_ID, IntakeTabs, openTabAccessibleName } from "./components/IntakeTabs.js";
export type { IntakeTabsProps } from "./components/IntakeTabs.js";
export {
  buildIntakeCreatePayload,
  emptyIntakeCreateForm,
  INTAKE_TITLE_MAX_LENGTH,
  intakeTitleError,
  isIntakeTitleSubmittable,
  normalizeIntakeTitle,
  resetIntakeCreateForm,
  visibleIntakeDescriptionHtml,
} from "./lib/createForm.js";
export type { IntakeCreateForm } from "./lib/createForm.js";
export { selectIntakeEmptyState } from "./lib/emptyStates.js";
export type { IntakeEmptyInput, IntakeEmptyState as IntakeEmptyKind } from "./lib/emptyStates.js";
export { intakeFeatureSettingsHref, intakeHref, retiredInboxHref } from "./lib/paths.js";
export { INTAKE_NAV_OVERLAY_SELECTOR, neighborIntakeId, suppressIntakeArrowKey } from "./lib/navigation.js";
export type { IntakeStepDirection } from "./lib/navigation.js";
export { isResolvedIntakeStatus, selectIntakeEditability, selectIntakeTriageVisibility } from "./lib/permissions.js";
export type {
  IntakeEditability,
  IntakeStanding,
  IntakeTriageStanding,
  IntakeTriageVisibility,
} from "./lib/permissions.js";
export { parseIntakeSearch } from "./lib/search.js";
export type { IntakeSearch, IntakeSearchWriteOptions } from "./lib/search.js";
export {
  TRIAGE_DELETE_LABEL,
  TRIAGE_DUPLICATE_LABEL,
  TRIAGE_REFUSED_TITLE,
  TRIAGE_SNOOZE_LABEL,
  TRIAGE_UNSNOOZE_LABEL,
  buildTriageMenuEntries,
  focusTriageLanding,
  scheduleTriageFocus,
  selectIntakeStatusChip,
  snoozeDayFromTimestamp,
  snoozeDaysLeft,
  snoozeTimestampForDay,
  triageRefusedMessage,
} from "./lib/triage.js";
export type {
  IntakeStatusChipModel,
  IntakeStatusTone,
  TriageMenuEntries,
  TriageMenuHandlers,
  TriageRefusedAction,
} from "./lib/triage.js";
export {
  DEFAULT_INTAKE_SEARCH,
  intakeSearchAfterRemove,
  intakeSearchFor,
  settleIntakeSearch,
  withoutSelection,
} from "./lib/searchWrites.js";
export { DEFAULT_INTAKE_TAB, INTAKE_TABS, parseIntakeTab } from "./lib/tabs.js";
export type { IntakeTab } from "./lib/tabs.js";
export {
  INTAKE_TAB_LABELS,
  INTAKE_TAB_STATUSES,
  shouldShowOpenBadge,
  siblingTab,
  statusParamForTab,
  statusSetForTab,
  tabIndex,
} from "./lib/tabsModel.js";
export { INTAKE_AREA_LABEL, intakeDocumentTitle } from "./lib/title.js";
