// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Paid-plan free-quota reminder (NEWPIDASH-181). A toggle in this section
// and one informational note per UTC day when the allowance runs out.
// Both render only on payg/member plans: free-plan users already get the
// 402 notice, and the OSS edition never mounts this tab. The note never
// blocks a turn, run, or composer — it is a polite status line beside the
// meter, with its toggle persisted server-side in Profile.settings.
import { freeQuotaReminderEnabled, updateProfile, type Profile, type ProfilePatch } from "@pidash/api-client";
import { Switch } from "@pidash/kit";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { applyOptimisticUpdate } from "../../../core/query/optimistic.js";
import { profileQueryOptions, sessionKeys } from "../../../core/session/queries.js";
import { notifyError } from "../../../shared/shell/index.js";
import { useFreeTokenUsage } from "../api/queries.js";
import {
  FREE_QUOTA_REMINDER_BODY,
  FREE_QUOTA_REMINDER_DISMISS,
  FREE_QUOTA_REMINDER_HELP,
  FREE_QUOTA_REMINDER_LABEL,
  FREE_QUOTA_REMINDER_SAVE_FAILED_MESSAGE,
  FREE_QUOTA_REMINDER_SAVE_FAILED_TITLE,
} from "../lib/copy.js";
import {
  isPaidPlan,
  markShownToday,
  platformStorage,
  shouldShowFreeQuotaReminder,
  utcDayKey,
} from "../lib/freeQuotaReminder.js";

/** Namespaced patch carrying one flag flip (merges server-side per namespace). */
export function freeQuotaReminderPatch(enabled: boolean): ProfilePatch {
  return { settings: { openhub: { free_quota_reminder: enabled } } };
}

/**
 * Flip the flag inside a cached profile, preserving every sibling value.
 * Anything but a set profile reads through untouched — the write still
 * lands and the server's answered row replaces the cache.
 */
export function applyFreeQuotaReminderFlip(previous: Profile | undefined, enabled: boolean): Profile | undefined {
  if (previous === undefined) return undefined;
  const namespace = previous.settings["openhub"];
  const openhub = typeof namespace === "object" && namespace !== null ? (namespace as Record<string, unknown>) : {};
  return { ...previous, settings: { ...previous.settings, openhub: { ...openhub, free_quota_reminder: enabled } } };
}

/**
 * Persist one flag flip. Optimistic: the UI applies immediately, takes
 * the server's answered row on success, and rolls back with a notice
 * when the server refuses (including the pre-backend 400, where the
 * previous value is restored and the user is told). Callers keep the
 * switch inert via isPending while a write is in flight.
 */
export function useSetFreeQuotaReminderEnabled() {
  const queryClient = useQueryClient();
  return useMutation<Profile, unknown, boolean>({
    mutationFn: async (enabled: boolean) => {
      const rollback = applyOptimisticUpdate<Profile>(queryClient, sessionKeys.profile, (previous) =>
        applyFreeQuotaReminderFlip(previous, enabled)
      );
      try {
        const saved = await updateProfile(getAppClient(), freeQuotaReminderPatch(enabled));
        queryClient.setQueryData(sessionKeys.profile, saved);
        return saved;
      } catch (error) {
        rollback();
        notifyError(FREE_QUOTA_REMINDER_SAVE_FAILED_TITLE, FREE_QUOTA_REMINDER_SAVE_FAILED_MESSAGE);
        throw error;
      }
    },
  });
}

/**
 * The flag behind the switch: true/false once the profile loads,
 * undefined while it has not (the switch stays unmounted meanwhile).
 * The server defaults the key to false; anything but explicit true
 * reads as off, matching the stored default.
 */
export function useFreeQuotaReminderEnabled(): boolean | undefined {
  const profile = useQuery(profileQueryOptions(getAppClient()));
  if (profile.data === undefined) return undefined;
  return freeQuotaReminderEnabled(profile.data.settings);
}

/**
 * Toggle + once-per-day note. Renders nothing until both the profile and
 * the quota have read, and nothing at all off the paid plans — so a free
 * user, an unknown plan, or a failing quota read sees exactly the
 * current silent behavior. The note carries role="status" (informational),
 * never an alert, and dismissing it suppresses it for the rest of the day.
 * Showing the note marks the day in storage, so a second visit the same
 * UTC day stays quiet while the first visit still renders it.
 */
export function FreeQuotaReminder(): React.ReactElement | null {
  const enabled = useFreeQuotaReminderEnabled();
  const quota = useFreeTokenUsage();
  const setEnabled = useSetFreeQuotaReminderEnabled();
  const [dismissed, setDismissed] = React.useState(false);

  const plan = quota.data?.plan ?? null;
  const used = quota.data?.llm.freeTokensUsed ?? null;
  const limit = quota.data?.llm.freeTokensLimit ?? null;
  const dayKey = utcDayKey(new Date());
  const showNote =
    enabled === true &&
    quota.data !== undefined &&
    !dismissed &&
    shouldShowFreeQuotaReminder({ enabled: true, plan, used, limit, storage: platformStorage(), dayKey });

  React.useEffect(() => {
    if (showNote) markShownToday(platformStorage(), dayKey);
  }, [showNote, dayKey]);

  if (enabled === undefined || quota.data === undefined) return null;
  if (!isPaidPlan(plan)) return null;

  const dismiss = (): void => {
    markShownToday(platformStorage(), dayKey);
    setDismissed(true);
  };

  return (
    <section
      aria-label="Free tokens reminder"
      className="flex flex-col gap-(--space-3)"
      data-testid="free-quota-reminder"
    >
      <div className="flex items-center justify-between gap-(--space-4)">
        <div className="flex flex-col gap-(--space-1)">
          <span className="text-body text-(--text)">{FREE_QUOTA_REMINDER_LABEL}</span>
          <span className="text-body text-(--text-muted)">{FREE_QUOTA_REMINDER_HELP}</span>
        </div>
        <Switch
          label={FREE_QUOTA_REMINDER_LABEL}
          checked={enabled}
          disabled={setEnabled.isPending}
          onCheckedChange={(next) => setEnabled.mutate(next)}
          data-testid="free-quota-reminder-switch"
        />
      </div>
      {showNote ? (
        <div
          role="status"
          className="flex items-center justify-between gap-(--space-4) rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-4) py-(--space-3)"
          data-testid="free-quota-reminder-banner"
        >
          <p className="text-body text-(--text)">{FREE_QUOTA_REMINDER_BODY}</p>
          <button
            type="button"
            className="text-body shrink-0 rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) hover:text-(--text)"
            onClick={dismiss}
            data-testid="free-quota-reminder-dismiss"
          >
            {FREE_QUOTA_REMINDER_DISMISS}
          </button>
        </div>
      ) : null}
    </section>
  );
}
