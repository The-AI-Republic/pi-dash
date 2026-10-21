// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the automations feature. Other features and routes
// import only from here.

export {
  AUTOMATION_UPDATE_FAILED_MESSAGE,
  AUTOMATION_UPDATE_FAILED_TITLE,
  AUTOMATIONS_NOT_AUTHORIZED_TITLE,
  automationKeys,
  automationStatesQueryOptions,
  useUpdateAutomation,
} from "./api/automationQueries.js";
export type { AutomationPatch } from "./api/automationQueries.js";
export {
  DELAY_PRESETS,
  DELAY_RANGE_MESSAGE,
  delayLabel,
  isDelayEnabled,
  MAX_DELAY_MONTHS,
  MIN_DELAY_MONTHS,
  parseDelayInput,
} from "./api/months.js";
export {
  archiveTogglePatch,
  closeTogglePatch,
  isStatePickerEnabled,
  selectCancelledStates,
  selectCloseTarget,
} from "./lib/rows.js";
export {
  AUTOMATIONS_EXTENSION_SLOT,
  AUTOMATIONS_HEADING,
  AUTOMATIONS_INTRO,
  AUTOMATIONS_LOADING_LABEL,
  AUTOMATIONS_UNAVAILABLE_MESSAGE,
  AutomationsScreen,
  AutomationsSettings,
  AutomationsSkeleton,
  NotAuthorizedView,
} from "./components/AutomationsSettings.js";
export type { AutomationsScreenProps, AutomationsSettingsProps } from "./components/AutomationsSettings.js";
export {
  ARCHIVE_DELAY_LEAD,
  ARCHIVE_ROW_DESCRIPTION,
  ARCHIVE_ROW_TITLE,
  AutomationRows,
  CLOSE_DELAY_LEAD,
  CLOSE_ROW_DESCRIPTION,
  CLOSE_ROW_TITLE,
  CLOSE_STATE_LEAD,
} from "./components/AutomationRows.js";
export type { AutomationRowsProps } from "./components/AutomationRows.js";
export { AutomationRow } from "./components/AutomationRow.js";
export type { AutomationRowProps } from "./components/AutomationRow.js";
export { CloseTargetPicker, STATE_FALLBACK_LABEL, STATE_PICKER_LIST_LABEL } from "./components/CloseTargetPicker.js";
export type { CloseTargetPickerProps } from "./components/CloseTargetPicker.js";
export { CUSTOM_MONTH_SEED, CUSTOM_MONTH_TITLE, CustomMonthDialog } from "./components/CustomMonthDialog.js";
export type { CustomMonthDialogProps } from "./components/CustomMonthDialog.js";
export { CUSTOMIZE_DELAY_LABEL, DelayPicker } from "./components/DelayPicker.js";
export type { DelayPickerProps } from "./components/DelayPicker.js";
