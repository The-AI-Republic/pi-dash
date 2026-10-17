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
export { unreadNotificationsQueryOptions } from "./api/queries.js";
