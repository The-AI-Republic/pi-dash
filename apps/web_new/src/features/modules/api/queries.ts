// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module reads the list shell owns (parity/specs/modules.md §2.1, §3.1,
// §13.1). The list is one bare-array read per project; its previous data
// stays on screen while a background refetch runs so a refresh never
// brings back the skeleton. Detail, links and work-item reads belong to the
// slices that render them; their keys already live in keys.ts.

import { queryOptions } from "@tanstack/react-query";
import { listProjectModules, type ApiClient } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { moduleKeys } from "./keys.js";

/** The project's non-archived modules: full rows for the list, board and timeline. */
export function modulesListQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: moduleKeys.list(workspaceSlug, projectId),
    queryFn: () => listProjectModules(client, workspaceSlug, projectId),
    staleTime: LIST_STALE_TIME_MS,
    // Fetch on mount only (§8.5): another member's change appears on the
    // next load, not behind the user's back.
    refetchOnWindowFocus: false,
  });
}
