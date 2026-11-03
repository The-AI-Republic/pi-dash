// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the notifications feature. Other features import only from here.

export { notificationKeys } from "./api/keys.js";
export {
  applyRowToPage,
  MARKED_ARCHIVED_TITLE,
  MARKED_READ_TITLE,
  MARKED_UNREAD_TITLE,
  mergeWrittenRow,
  rowBelongsToStream,
  SNOOZED_TITLE,
  UNARCHIVED_TITLE,
  UNSNOOZED_TITLE,
  useMarkAllRead,
  useSnoozeNotification,
  useToggleNotificationArchive,
  useToggleNotificationRead,
} from "./api/mutations.js";
export type { ArchiveToggle, BulkReadInput, ReadToggle, SnoozeWrite, StreamFlags } from "./api/mutations.js";
export { notificationListQueryFromKey } from "./api/streamQuery.js";
export {
  applyEmailPreferenceFlip,
  EMAIL_PREFS_FAILED_TITLE,
  EMAIL_PREFS_SAVED_TITLE,
  emailPreferencePatch,
  emailPreferencesQueryOptions,
  saveFailureMessage,
  useEmailPreferences,
  useUpdateEmailPreference,
} from "./api/preferences.js";
export type { EmailPreferenceFlip, EmailPreferenceTopic } from "./api/preferences.js";
export {
  COMPLETED_ONLY_ROW,
  EMAIL_PREFERENCE_ROWS,
  EMAIL_PREFS_HEADING,
  EMAIL_PREFS_INTRO,
  EMAIL_PREFS_LOAD_FAILED,
  EMAIL_PREFS_LOADING_LABEL,
  EmailPreferencesPage,
  pendingFlipTopic,
} from "./components/EmailPreferencesPage.js";
export type { EmailPreferenceRow, PendingFlip } from "./components/EmailPreferencesPage.js";
export { applyBulkRead, isRowInBulkReadScope, markPageRead } from "./lib/bulkRead.js";
export type { CachedListPage } from "./lib/bulkRead.js";
export { useBackgroundFetchGauge } from "./lib/useBackgroundFetch.js";
export { useStreamReset } from "./lib/useStreamReset.js";
export type { BackgroundFetchGauge } from "./lib/useBackgroundFetch.js";
export { OriginChips } from "./components/OriginChips.js";
export type { OriginChipsProps } from "./components/OriginChips.js";
export { StreamControls } from "./components/StreamControls.js";
export type { StreamControlsProps } from "./components/StreamControls.js";
export {
  DEFAULT_STREAM_MODES,
  isHiddenByStreamMode,
  isModeEnabled,
  markAllReadScope,
  notificationStreamQuery,
  NOTIFICATION_MODES,
  NOTIFICATION_MODE_LABELS,
  NOTIFICATION_ORIGINS,
  NOTIFICATION_ORIGIN_LABELS,
  removeOrigin,
  toggleMode,
  toggleOrigin,
} from "./lib/stream.js";
export type { NotificationOrigin, OriginFilter, StreamMode, StreamModes } from "./lib/stream.js";
export {
  NOTIFICATIONS_FIRST_CURSOR,
  NOTIFICATIONS_PAGE_SIZE,
  nextNotificationCursor,
  notificationPagesQueryOptions,
  notificationsListQueryOptions,
  useNotificationPages,
  unreadNotificationsQueryOptions,
  useNotificationsList,
  useUnreadNotifications,
} from "./api/queries.js";
export {
  applyReadToNotificationCache,
  applyReadToTotals,
  applySuccessfulRead,
  useMarkNotificationRead,
} from "./api/read.js";
export type { MarkNotificationReadInput } from "./api/read.js";
export { InboxShell } from "./components/InboxShell.js";
export { InboxView } from "./components/InboxView.js";
export { NotificationDetailHost } from "./components/NotificationDetailHost.js";
export type { NotificationDetailHostProps } from "./components/NotificationDetailHost.js";
export { NotificationEmptyState, NotificationListSkeleton, NotificationNextPage } from "./components/ListStates.js";
export { CUSTOM_SNOOZE_ENTRY, REMOVE_SNOOZE_ENTRY, SnoozeMenu } from "./components/SnoozeMenu.js";
export type { SnoozeMenuProps } from "./components/SnoozeMenu.js";
export {
  CUSTOM_SNOOZE_CANCEL,
  CUSTOM_SNOOZE_SUBMIT,
  CustomSnoozeDialog,
  DATE_PLACEHOLDER,
  DATE_SECTION_HEADING,
  NO_SLOTS_LEFT,
  TIME_PLACEHOLDER,
  TIME_SECTION_HEADING,
} from "./components/CustomSnoozeDialog.js";
export type { CustomSnoozeDialogProps, Meridiem } from "./components/CustomSnoozeDialog.js";
export { NotificationCard } from "./components/NotificationCard.js";
export type { NotificationCardActions, NotificationCardProps } from "./components/NotificationCard.js";
export { StreamTabs } from "./components/StreamTabs.js";
export { selectUnreadBadgeCount } from "./lib/badgeCount.js";
export { describeNotificationChange, resolveNotificationCard, resolveNotificationCards } from "./lib/card.js";
export type { NotificationCardData, NotificationChange } from "./lib/card.js";
export { formatNotificationAge } from "./lib/relativeAge.js";
export {
  SNOOZE_PRESETS,
  composeResumeInstant,
  dayIsFullyElapsed,
  earliestSnoozeDay,
  formatResumeLabel,
  offeredSlots,
  pickedDayLabel,
  presetResumeInstant,
  slotLabels,
  slotTo24Hour,
  SLOTS_PER_MERIDIEM,
  slotToMinutes,
  snoozePresetByLabel,
} from "./lib/snooze.js";
export type { SnoozePreset } from "./lib/snooze.js";
export { NOTIFICATION_TABS, notificationTabQuery } from "./lib/stream.js";
export type { NotificationTab } from "./lib/stream.js";
