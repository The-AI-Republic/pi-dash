// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timezone picker shaping (shell spec, SHELL-097). Option grouping and
// filtering live here; the zone list query and the account write are
// the shared preference module's, re-exported so picker callers have
// one import. The palette's Change-timezone page and the profile
// timezone section share the same cache and mutation.

import { useQuery } from "@tanstack/react-query";
import type { TimezoneEntry } from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { timezonesQueryOptions, useUpdateTimezone } from "../shell/preferences.js";

export { timezonesQueryOptions, useUpdateTimezone };
export type { TimezoneMutationHandlers as UpdateTimezoneHandlers } from "../shell/preferences.js";

/** Read the zone list through the app client. */
export function useTimezones() {
  return useQuery(timezonesQueryOptions(getAppClient()));
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
