// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Week-start read for the date pickers (parity/specs/issues.md §17,
// ISS-214). The profile's start_of_the_week drives the calendar header
// order; Sunday while loading or when the profile carries none.

import { useQuery } from "@tanstack/react-query";

import { getAppClient } from "../../../core/api/client.js";
import { profileQueryOptions } from "../../../core/session/queries.js";
import { DEFAULT_WEEK_START, normalizeWeekStart } from "./calendar.js";

/** The signed-in user's first day of week, Sunday-first 0-6. */
export function useWeekStart(): number {
  const profile = useQuery(profileQueryOptions(getAppClient()));
  if (profile.data === undefined) return DEFAULT_WEEK_START;
  return normalizeWeekStart(profile.data.start_of_the_week);
}
