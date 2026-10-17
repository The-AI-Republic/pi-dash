// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the notifications feature. Other features import only from here.

export { notificationKeys } from "./api/keys.js";
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
export {
  NOTIFICATIONS_FIRST_CURSOR,
  NOTIFICATIONS_PAGE_SIZE,
  notificationsListQueryOptions,
  unreadNotificationsQueryOptions,
  useNotificationsList,
  useUnreadNotifications,
} from "./api/queries.js";
export { InboxShell } from "./components/InboxShell.js";
export { InboxView } from "./components/InboxView.js";
export { NotificationCard } from "./components/NotificationCard.js";
export { StreamTabs } from "./components/StreamTabs.js";
export { selectUnreadBadgeCount } from "./lib/badgeCount.js";
export { describeNotificationChange, resolveNotificationCard, resolveNotificationCards } from "./lib/card.js";
export type { NotificationCardData, NotificationChange } from "./lib/card.js";
export { formatNotificationAge } from "./lib/relativeAge.js";
export { NOTIFICATION_TABS, notificationTabQuery } from "./lib/stream.js";
export type { NotificationTab } from "./lib/stream.js";
