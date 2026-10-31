// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Widget-map queries (SHELL-018–023; written from parity/specs/shell.md
// §2). The home-preferences map is the widget stack's source of order and
// visibility. It shares the home rules: no refetch on focus or reconnect
// (SHELL-023), failures logged with prior state kept and no retry
// affordance. Toggles and reorders belong to NEWFRONT-279, which reuses
// this read.

import { queryOptions, useQuery } from "@tanstack/react-query";
import { listHomePreferences, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS, workspaceScope } from "../../../core/query/client.js";
import { logHomeFetch } from "./fetch.js";

export const widgetPrefsKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "home", "widget-prefs"] as const,
  list: (workspaceSlug: string) => [...widgetPrefsKeys.all(workspaceSlug), "list"] as const,
};

export function widgetPrefsQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: widgetPrefsKeys.list(workspaceSlug),
    queryFn: () => logHomeFetch(`widget-prefs/${workspaceSlug}`, () => listHomePreferences(client, workspaceSlug)),
    staleTime: LIST_STALE_TIME_MS,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

/** The widget map for one workspace: order plus visibility per key. */
export function useWidgetPrefs(workspaceSlug: string) {
  return useQuery(widgetPrefsQueryOptions(getAppClient(), workspaceSlug));
}
