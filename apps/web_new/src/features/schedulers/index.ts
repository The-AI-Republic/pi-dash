// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public scheduler feature API. Later slices import only from here.

export {
  schedulerFieldError,
  schedulerKeys,
  schedulersQueryOptions,
  useCreateScheduler,
  useDeleteScheduler,
  useSchedulers,
  useUpdateScheduler,
} from "./api.js";
export type { SchedulerUpdateRequest } from "./api.js";
export {
  DEFAULT_SCHEDULER_COLOR,
  deriveSchedulerHandle,
  emptySchedulerDefinition,
  hasSchedulerDefinitionErrors,
  validateSchedulerDefinition,
} from "./definition.js";
export type { SchedulerDefinitionErrors, SchedulerDefinitionValue } from "./definition.js";
export { SchedulerCatalog, formatSchedulerUpdatedAt, schedulerSourceLabel } from "./SchedulerCatalog.js";
export type { SchedulerCatalogProps } from "./SchedulerCatalog.js";
export { SchedulerDefinitionDialog } from "./SchedulerDefinitionDialog.js";
export type { SchedulerDefinitionDialogProps } from "./SchedulerDefinitionDialog.js";
export { SchedulerDeleteDialog } from "./SchedulerDeleteDialog.js";
export type { SchedulerDeleteDialogProps } from "./SchedulerDeleteDialog.js";

// Project installs (AGT-006–012).
export {
  bindingDetailQueryOptions,
  bindingKeys,
  bindingRunsQueryOptions,
  bindingsQueryOptions,
  invalidateInstalls,
  RUNS_POLL_INTERVAL_MS,
  useSchedulerBindingDetail,
  useSchedulerBindingRuns,
  schedulerBindingFailure,
  schedulerCatalogKey,
  useInstallSchedulerBinding,
  useSchedulerBindings,
  useToggleSchedulerBinding,
  useUninstallSchedulerBinding,
  useUpdateSchedulerBinding,
} from "./installData.js";
export type {
  InstallBindingRequest,
  ToggleBindingRequest,
  UninstallBindingRequest,
  UpdateBindingRequest,
} from "./installData.js";
export {
  DEFAULT_TZID,
  browserTimeZone,
  defaultStartLocal,
  describeRrule,
  describeStoredRrule,
  emptySchedule,
  hasScheduleErrors,
  isoToLocalStart,
  localStartToIso,
  schedulePayload,
  timeZoneOptions,
  validateSchedule,
} from "./schedule.js";
export type { ScheduleErrors, SchedulePayload, ScheduleValue } from "./schedule.js";
export {
  DEFAULT_AFTER_COUNT,
  defaultRecurrence,
  parseRecurrence,
  serializeRecurrence,
  withMonthlyMode,
  withUnit,
  withWeekdayToggled,
} from "./recurrence.js";
export type { ParsedRecurrence, RecurrenceState, RepeatUnit } from "./recurrence.js";
export { RecurrenceBuilder } from "./RecurrenceBuilder.js";
export type { RecurrenceBuilderProps } from "./RecurrenceBuilder.js";
export { ScheduleFields, scheduleFieldIds, useScheduleDraft } from "./ScheduleFields.js";
export type { ScheduleFieldsProps } from "./ScheduleFields.js";
export { DEFAULT_OUTCOME_MODE, OUTCOME_OPTIONS, OutcomeModeField, outcomeHelp } from "./OutcomeModeField.js";
export type { OutcomeMode, OutcomeModeFieldProps } from "./OutcomeModeField.js";
export {
  DEFAULT_POD_VALUE,
  PodField,
  podKeys,
  podLabel,
  projectPodsQueryOptions,
  selectedPodValue,
} from "./PodField.js";
export type { PodFieldProps } from "./PodField.js";
export {
  bulkFailureTitle,
  bulkSelectionSummary,
  bulkSuccessTitle,
  eligibleDefinitions,
  filterProjects,
  failureSummary,
  partitionInstallOutcomes,
  toggleAllBulkTargets,
  toggleBulkTarget,
} from "./installResults.js";
export type { BulkSelection, InstallOutcome, InstallPartition } from "./installResults.js";
export { InstallsList, formatRunTimestamp, scheduleText } from "./InstallsList.js";
export {
  configRows,
  displayTimeZone,
  emptyHistoryText,
  formatDuration,
  formatMoment,
  installedByName,
  isSeriesCompleted,
  pagerTotalPages,
  runResultExcerpt,
  runStartedLabel,
  runStatusLabel,
  runStatusTone,
  SERIES_COMPLETED_BADGE,
  SERIES_COMPLETED_TEXT,
  SERIES_EXHAUSTED_FIELD,
  sourceLabel,
} from "./bindingDetail.js";
export type { ConfigRow, RunTone } from "./bindingDetail.js";
export type { InstallsListProps } from "./InstallsList.js";
export {
  InstallDialog,
  InstallForm,
  definitionFieldError,
  failureMessage,
  initialInstallMode,
  validateDefinitionDraft,
} from "./InstallDialog.js";
export type { InstallDialogProps, InstallMode } from "./InstallDialog.js";
export { BulkInstallDialog, joinedProjects } from "./BulkInstallDialog.js";
export type { BulkInstallDialogProps } from "./BulkInstallDialog.js";
export { EditInstallDialog, installDraftFromBinding, installUpdatePayload } from "./EditInstallDialog.js";
export type { EditInstallDialogProps, InstallDraft } from "./EditInstallDialog.js";
export { UninstallInstallDialog, uninstallConsequence } from "./UninstallInstallDialog.js";
export type { UninstallInstallDialogProps } from "./UninstallInstallDialog.js";

// Firing calendar, visibility rail and occurrence panel (AGT-015–017, 062).
export {
  MAX_WINDOW_DAYS,
  TRUNCATION_NOTICE,
  clampWindow,
  occurrenceKeys,
  occurrencesQueryOptions,
  useSchedulerOccurrences,
} from "./occurrences.js";
export {
  DAY_HEIGHT,
  HOUR_HEIGHT,
  MONTH_BLOCKS_PER_DAY,
  addDays,
  dayKey,
  daysBetween,
  formatBlockTime,
  formatDayLabel,
  formatMonthYear,
  monthCells,
  assignWeekLanes,
  monthWindow,
  nextMonth,
  occurrenceTop,
  occurrencesByDay,
  planMonthDay,
  previousMonth,
  rollupDay,
  startOfMonth,
  startOfNextDay,
  startOfWeek,
  timeLineTop,
  weekDays,
  weekWindow,
} from "./calendarModel.js";
export type { MonthCell, MonthDayPlan, MonthRollup } from "./calendarModel.js";
export {
  formatOccurrenceInstant,
  occurrenceContext,
  occurrenceRecurrence,
  occurrenceRunHref,
  occurrenceStatus,
} from "./calendarText.js";
export {
  readHiddenSchedulers,
  useSchedulerVisibility,
  visibilityStorageKey,
  writeHiddenSchedulers,
} from "./visibility.js";
export type { VisibilityState } from "./visibility.js";
export { VisibilityRail, railCalendars } from "./VisibilityRail.js";
export type { RailCalendar, VisibilityRailProps } from "./VisibilityRail.js";
export type { OccurrenceWindow } from "./occurrences.js";
export { CalendarWeekView } from "./CalendarWeekView.js";
export type { CalendarWeekViewProps } from "./CalendarWeekView.js";
export { CalendarMonthView } from "./CalendarMonthView.js";
export type { CalendarMonthViewProps } from "./CalendarMonthView.js";
export { OccurrencePanel } from "./OccurrencePanel.js";
export type { OccurrencePanelProps } from "./OccurrencePanel.js";
export { CLOCK_TICK_MS, FiringCalendar } from "./FiringCalendar.js";
export type { CalendarView, FiringCalendarProps } from "./FiringCalendar.js";
