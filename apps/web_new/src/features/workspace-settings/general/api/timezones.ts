// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Server timezone list for the general form's picker
// (parity/specs/workspace-settings.md §3; WSET-003). One read per mount,
// shared across the area, turned into merged picker choices by rules.ts.

import { queryOptions, useQuery } from "@tanstack/react-query";

import { listTimezones, type ApiClient, type TimezoneList } from "@pidash/api-client";
import { REFERENCE_STALE_TIME_MS } from "../../../../core/query/index.js";
import { getAppClient } from "../../../../core/api/client.js";

export const timezoneKeys = {
  list: ["workspace-settings", "timezones"] as const,
};

export function timezoneListQueryOptions(client: ApiClient) {
  return queryOptions<TimezoneList>({
    queryKey: timezoneKeys.list,
    queryFn: () => listTimezones(client),
    staleTime: REFERENCE_STALE_TIME_MS,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

export function useTimezoneList() {
  return useQuery(timezoneListQueryOptions(getAppClient()));
}
