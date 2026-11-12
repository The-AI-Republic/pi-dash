// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-states reads for recent-row status names (SHELL-017; written
// from parity/specs/shell.md §2). Reference data with the home rules: no
// refetch on focus or reconnect, failures logged. A failed read degrades
// the row (its status marker is omitted), never an error surface.

import { queryOptions } from "@tanstack/react-query";
import { listStates, type ApiClient } from "@pidash/api-client";

import { REFERENCE_STALE_TIME_MS, workspaceScope } from "../../../core/query/client.js";
import { logHomeFetch } from "./fetch.js";

export const projectStatesKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "home", "states"] as const,
  list: (workspaceSlug: string, projectId: string) => [...projectStatesKeys.all(workspaceSlug), projectId] as const,
};

export function projectStatesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: projectStatesKeys.list(workspaceSlug, projectId),
    queryFn: () =>
      logHomeFetch(`states/${workspaceSlug}/${projectId}`, () => listStates(client, workspaceSlug, projectId)),
    staleTime: REFERENCE_STALE_TIME_MS,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}
