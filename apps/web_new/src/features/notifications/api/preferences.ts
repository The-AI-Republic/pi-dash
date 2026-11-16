// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Email delivery preferences (parity/specs/notifications.md §10,
// NTF-024/025/031). One toggle per topic, each saving instantly and
// independently: flipping a toggle optimistically moves it, PATCHes just
// that key, and reports a toast. A failed save rolls the toggle back to
// the persisted value (the NEWFRONT-211 target) with an error toast; the
// frontend's job ends at persisting the toggle — the mailer consumes the
// flags (NTF-025, §12). No list code lives here, so there is no inline
// list-error panel to build (NTF-031).

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  getNotificationPreferences,
  updateNotificationPreferences,
  type ApiClient,
  type NotificationPreferences,
  type NotificationPreferencePatch,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { applyOptimisticUpdate } from "../../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { notificationKeys } from "./keys.js";

/** One email topic the settings page toggles. */
export type EmailPreferenceTopic = "property_change" | "state_change" | "comment" | "mention" | "issue_completed";

/** One instant save: the flipped topic and its next value. */
export interface EmailPreferenceFlip {
  topic: EmailPreferenceTopic;
  enabled: boolean;
}

/** The PATCH body for one flip: the single changed key. */
export function emailPreferencePatch(flip: EmailPreferenceFlip): NotificationPreferencePatch {
  return { [flip.topic]: flip.enabled };
}

/** Flip one topic over the cached row, leaving the rest untouched. */
export function applyEmailPreferenceFlip(
  previous: NotificationPreferences | undefined,
  flip: EmailPreferenceFlip
): NotificationPreferences | undefined {
  if (previous === undefined) return undefined;
  return { ...previous, [flip.topic]: flip.enabled };
}

/** The caller's email delivery preferences. */
export function emailPreferencesQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: notificationKeys.emailPreferences,
    queryFn: () => getNotificationPreferences(client),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

/** The caller's email delivery preferences. */
export function useEmailPreferences() {
  return useQuery(emailPreferencesQueryOptions(getAppClient()));
}

export const EMAIL_PREFS_SAVED_TITLE = "Email preferences updated successfully";
export const EMAIL_PREFS_FAILED_TITLE = "Failed to update email preferences";

/** Human detail for a failed save, when the failure carries one. */
export function saveFailureMessage(error: unknown): string | undefined {
  return error instanceof Error ? error.message : undefined;
}

/**
 * Persist one toggle flip: the toggle moves at once, the single changed
 * key PATCHes, and the cache takes the server's answered row. A failed
 * save restores the persisted value with an error toast.
 */
export function useUpdateEmailPreference() {
  const queryClient = useQueryClient();
  const key = notificationKeys.emailPreferences;
  return useMutation<NotificationPreferences, Error, EmailPreferenceFlip>({
    mutationFn: async (flip) => {
      const rollback = applyOptimisticUpdate<NotificationPreferences>(queryClient, key, (previous) =>
        applyEmailPreferenceFlip(previous, flip)
      );
      try {
        const saved = await updateNotificationPreferences(getAppClient(), emailPreferencePatch(flip));
        queryClient.setQueryData(key, saved);
        notifySuccess(EMAIL_PREFS_SAVED_TITLE);
        return saved;
      } catch (error) {
        rollback();
        notifyError(EMAIL_PREFS_FAILED_TITLE, saveFailureMessage(error));
        throw error;
      }
    },
  });
}
