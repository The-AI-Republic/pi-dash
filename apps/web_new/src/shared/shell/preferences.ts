// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Preference reads and writes (shell spec, SHELL-088). The palette's four
// preference entries and their sub-pages run through here; the appearance
// surfaces (NEWFRONT-294, SHELL-095 through SHELL-097) reuse the same
// mutations and option lists instead of forking them. Theme applies
// immediately, persists to the profile and reloads the app; the other
// three persist with confirmation toasts and error toasts on failure.

import { useMutation, useQueryClient, queryOptions } from "@tanstack/react-query";
import { applyTheme, type ThemeMode } from "@pidash/kit";
import { listTimezones, updateMyTimezone, updateProfile, type ApiClient, type Profile } from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../core/query/client.js";
import { sessionKeys } from "../../core/session/queries.js";
import { notifyError, notifySuccess } from "./toaster.js";

/** One labeled option on a preference sub-page. */
export interface PreferenceChoice<T> {
  value: T;
  label: string;
}

/**
 * Theme options the palette offers (SHELL-088). The kit engine applies
 * system, light and dark today; the high-contrast pair and custom palettes
 * join this list with the appearance slice (NEWFRONT-294, SHELL-095).
 */
export const THEME_CHOICES: Array<PreferenceChoice<ThemeMode>> = [
  { value: "system", label: "System preference" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

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

/**
 * Language options the palette offers (SHELL-088). The open-source build
 * ships English only; fuller builds expand this list with the language
 * slice (NEWFRONT-294, SHELL-096).
 */
export const LANGUAGE_CHOICES: Array<PreferenceChoice<string>> = [{ value: "en", label: "English" }];

export const THEME_UPDATED_TITLE = "Theme updated";
export const THEME_UPDATED_MESSAGE = "Reloading to apply it.";
export const THEME_FAILED_TITLE = "Couldn't update the theme";
export const PREFERENCES_UPDATED_TITLE = "Preferences updated";
export const PREFERENCES_FAILED_TITLE = "Couldn't update preferences";
export const TIMEZONE_UPDATED_TITLE = "Timezone updated";
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

/** Reloads the running app. Tests inject a spy through the hook options. */
export function reloadApp(): void {
  window.location.reload();
}

function failureMessage(error: unknown): string {
  return (error as Error).message;
}

/**
 * Switch the interface theme (SHELL-088): applies at once, persists the
 * merged theme bag to the profile, then reloads so every surface picks
 * the choice up. Boot-time sync of the saved choice belongs to the
 * appearance slice (NEWFRONT-294, SHELL-095).
 */
export function useUpdateTheme(options?: { reload?: () => void }) {
  const queryClient = useQueryClient();
  const reload = options?.reload ?? reloadApp;
  return useMutation<unknown, Error, { theme: ThemeMode }>({
    mutationFn: async (input) => {
      applyTheme(input.theme);
      const client = getAppClient();
      const cached = queryClient.getQueryData<Profile>(sessionKeys.profile);
      try {
        const profile = await updateProfile(client, {
          theme: { ...(cached?.theme ?? {}), theme: input.theme },
        });
        notifySuccess(THEME_UPDATED_TITLE, THEME_UPDATED_MESSAGE);
        reload();
        return profile;
      } catch (error) {
        notifyError(THEME_FAILED_TITLE, failureMessage(error));
        throw error;
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: sessionKeys.profile });
    },
  });
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

/**
 * Persist the interface language to the profile (SHELL-088). The document
 * language flips at once; the full shell re-render belongs to the
 * language slice (NEWFRONT-294, SHELL-096).
 */
export function useUpdateLanguage() {
  const queryClient = useQueryClient();
  return useMutation<unknown, Error, { language: string }>({
    mutationFn: async (input) => {
      const client = getAppClient();
      try {
        const profile = await updateProfile(client, { language: input.language });
        document.documentElement.lang = input.language;
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

/** Persist the account timezone to the user row (SHELL-088). */
export function useUpdateTimezone() {
  const queryClient = useQueryClient();
  return useMutation<unknown, Error, { timezone: string }>({
    mutationFn: async (input) => {
      const client = getAppClient();
      try {
        const user = await updateMyTimezone(client, input.timezone);
        notifySuccess(TIMEZONE_UPDATED_TITLE);
        return user;
      } catch (error) {
        notifyError(TIMEZONE_FAILED_TITLE, failureMessage(error));
        throw error;
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: sessionKeys.me });
    },
  });
}
