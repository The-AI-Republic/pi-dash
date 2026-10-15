// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timezone preference data (shell spec, SHELL-097). The zone list is
// global reference data; the update writes the account's user_timezone
// with the confirmed toast. The palette's Change-timezone page and the
// profile timezone section share this module.

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { listTimezones, updateMyTimezone, type ApiClient, type TimezoneEntry } from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../core/query/client.js";
import { sessionKeys } from "../../core/session/queries.js";
import type { Me } from "@pidash/api-client";
import { notifyError, notifySuccess } from "../shell/toaster.js";

export const timezoneKeys = {
  all: ["timezones"] as const,
  list: ["timezones", "list"] as const,
};

/** The full zone list. Global reference data: long stale, no refetch. */
export function timezoneListQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: timezoneKeys.list,
    queryFn: () => listTimezones(client),
    staleTime: REFERENCE_STALE_TIME_MS,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

/** Read the zone list through the app client. */
export function useTimezones() {
  return useQuery(timezoneListQueryOptions(getAppClient()));
}

export interface UpdateTimezoneHandlers {
  onSuccess?: (timezone: string) => void;
  onError?: (error: Error) => void;
}

/**
 * Persist the account timezone. Confirms with the pinned toast, keeps
 * the signed-in user cache in sync, and toasts failures; the handlers
 * let callers react on top (the palette page closes on success).
 */
export function useUpdateTimezone(handlers: UpdateTimezoneHandlers = {}) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (timezone: string) => updateMyTimezone(getAppClient(), timezone),
    onSuccess: (row, timezone) => {
      queryClient.setQueryData<Me | undefined>(sessionKeys.me, (previous) =>
        previous === undefined ? previous : { ...previous, user_timezone: row.user_timezone }
      );
      notifySuccess("Timezone updated successfully.");
      handlers.onSuccess?.(timezone);
    },
    onError: (error: Error) => {
      notifyError("Couldn't update the timezone", error.message);
      handlers.onError?.(error);
    },
  });
}

/** One selectable zone row: duplicate values merge into one option. */
export interface TimezoneOption {
  value: string;
  label: string;
  detail: string;
  query: string;
}

/**
 * Group raw entries by value (labels join with a comma), sort by offset
 * label, and append the UTC shortcut. Mirrors the old picker's option
 * shaping so searches behave the same.
 */
export function selectTimezoneOptions(entries: TimezoneEntry[]): TimezoneOption[] {
  const seen = new Map<string, { labels: string[]; entry: TimezoneEntry }>();
  for (const entry of entries) {
    const known = seen.get(entry.value);
    if (known === undefined) seen.set(entry.value, { labels: [entry.label], entry });
    else if (!known.labels.includes(entry.label)) known.labels.push(entry.label);
  }
  const options = [...seen.values()].map(({ labels, entry }) => ({
    value: entry.value,
    label: labels.join(", "),
    detail: `${entry.utc_offset} ${entry.label}`,
    query: `${entry.value} ${labels.join(", ")}, ${entry.gmt_offset}, ${entry.utc_offset}`.toLowerCase(),
  }));
  options.sort((a, b) => a.detail.localeCompare(b.detail));
  options.push({
    value: "UTC",
    label: "UTC",
    detail: "UTC",
    query: "utc, coordinated universal time",
  });
  return options;
}

/** Case-insensitive substring match over the shaped query text. */
export function filterTimezoneOptions(options: TimezoneOption[], query: string): TimezoneOption[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return options;
  return options.filter((option) => option.query.includes(needle));
}
