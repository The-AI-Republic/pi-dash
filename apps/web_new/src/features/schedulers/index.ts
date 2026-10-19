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
export {
  DEFAULT_TZID,
  browserTimeZone,
  defaultStartLocal,
  describeRrule,
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
  bindingKeys,
  bindingsQueryOptions,
  invalidateInstalls,
  schedulerCatalogKey,
  useInstallSchedulerBinding,
  useSchedulerBindings,
  useToggleSchedulerBinding,
} from "./installData.js";
export type { InstallBindingRequest, ToggleBindingRequest } from "./installData.js";
export {
  bulkFailureTitle,
  bulkSelectionSummary,
  bulkSuccessTitle,
  eligibleDefinitions,
  filterProjects,
  firstFailureMessage,
  partitionInstallOutcomes,
  toggleAllBulkTargets,
  toggleBulkTarget,
} from "./installResults.js";
export type { BulkSelection, InstallOutcome, InstallPartition } from "./installResults.js";
export { ScheduleFields, scheduleFieldIds, useScheduleDraft } from "./ScheduleFields.js";
export type { ScheduleFieldsProps } from "./ScheduleFields.js";
export { InstallsList, formatRunTimestamp, scheduleText } from "./InstallsList.js";
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
