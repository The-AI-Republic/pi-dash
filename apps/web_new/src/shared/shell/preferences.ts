// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Preference reads and writes (shell spec, SHELL-088/095-097). The
// palette's four preference entries and their sub-pages run through
// here; the appearance surfaces reuse the same mutations and option
// lists instead of forking them. Theme and language picks go through
// their engines (core/theme setTheme, core/i18n setLanguage:
// immediate re-skin and re-render, dual persist, no reload); the
// other two persist with confirmation toasts and error toasts on
// failure.

import { useMutation, useQueryClient, queryOptions } from "@tanstack/react-query";
import { listTimezones, updateMyTimezone, updateProfile, type ApiClient, type Me } from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../core/query/client.js";
import { sessionKeys } from "../../core/session/queries.js";
import { THEME_OPTIONS, type ThemeValue } from "../../core/theme/index.js";
import { notifyError, notifySuccess } from "./toaster.js";

/** One labeled option on a preference sub-page. */
export interface PreferenceChoice<T> {
  value: T;
  label: string;
}

/**
 * Theme options the palette offers (SHELL-088/095): the engine's six
 * stored values, in display order. Single-sourced from core/theme so
 * the palette list and the engine apply the same values.
 */
export const THEME_CHOICES: Array<PreferenceChoice<ThemeValue>> = [...THEME_OPTIONS];

/**
 * First-day-of-week options (SHELL-088). Values are the server's 0-based
 * weekday index with Sunday at zero, matching start_of_the_week.
 */
export const FIRST_DAY_CHOICES: Array<PreferenceChoice<number>> = [
  { value: 0, label: "Sunday" },
  { value: 1, label: "Monday" },
  { value: 2, label: "Tuesday" },
  { value: 3, label: "Wednesday" },
  { value: 4, label: "Thursday" },
  { value: 5, label: "Friday" },
  { value: 6, label: "Saturday" },
];

export const PREFERENCES_UPDATED_TITLE = "Preferences updated";
export const PREFERENCES_FAILED_TITLE = "Couldn't update preferences";
/** Timezone confirmation, pinned by the SHELL-097 oracle scenario. */
export const TIMEZONE_UPDATED_TITLE = "Timezone updated successfully.";
export const TIMEZONE_FAILED_TITLE = "Couldn't update the timezone";

export const preferenceKeys = {
  timezones: ["preferences", "timezones"],
} as const;

/** The full zone list behind the timezone sub-page (SHELL-088/097). */
export function timezonesQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: preferenceKeys.timezones,
    queryFn: () => listTimezones(client),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

function failureMessage(error: unknown): string {
  return (error as Error).message;
}

/** Persist the first day of week to the profile (SHELL-088). */
export function useUpdateFirstDay() {
  const queryClient = useQueryClient();
  return useMutation<unknown, Error, { startOfWeek: number }>({
    mutationFn: async (input) => {
      const client = getAppClient();
      try {
        const profile = await updateProfile(client, { start_of_the_week: input.startOfWeek });
        notifySuccess(PREFERENCES_UPDATED_TITLE);
        return profile;
      } catch (error) {
        notifyError(PREFERENCES_FAILED_TITLE, failureMessage(error));
        throw error;
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: sessionKeys.profile });
    },
  });
}

/** Caller reactions on top of the timezone write (SHELL-097). */
export interface TimezoneMutationHandlers {
  onSuccess?: (timezone: string) => void;
  onError?: (error: Error) => void;
}

/**
 * Persist the account timezone to the user row (SHELL-088/097). The
 * single implementation behind the palette page and the pickers:
 * confirms with the pinned toast, syncs the signed-in user cache at
 * once, and toasts failures; the handlers let callers react on top.
 */
export function useUpdateTimezone(handlers: TimezoneMutationHandlers = {}) {
  const queryClient = useQueryClient();
  return useMutation<unknown, Error, { timezone: string }>({
    mutationFn: async (input) => {
      const client = getAppClient();
      try {
        const user = await updateMyTimezone(client, input.timezone);
        queryClient.setQueryData<Me | undefined>(sessionKeys.me, (previous) =>
          previous === undefined ? previous : { ...previous, user_timezone: user.user_timezone }
        );
        notifySuccess(TIMEZONE_UPDATED_TITLE);
        handlers.onSuccess?.(input.timezone);
        return user;
      } catch (error) {
        notifyError(TIMEZONE_FAILED_TITLE, failureMessage(error));
        handlers.onError?.(error as Error);
        throw error;
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: sessionKeys.me });
    },
  });
}
