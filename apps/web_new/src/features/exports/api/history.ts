// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Previous-exports history read (parity/specs/exports.md §3, EXP-006–010).
// One cursor window per hook instance, nested under the shared history
// prefix so the form's post-submit invalidation re-reads the mounted
// window (EXP-006). Polling follows the mounted rows: while any row is
// still processing the read repeats every few seconds, and it settles
// once none is (EXP-009). Failures surface inline next to the manual
// refresh, which doubles as the retry.
import { keepPreviousData, queryOptions, useQuery } from "@tanstack/react-query";
import { listWorkspaceExports, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { EXPORT_HISTORY_FIRST_CURSOR, EXPORT_HISTORY_PAGE_SIZE, historyQuery } from "../exportHistory.js";
import { exportsKeys } from "./keys.js";

export function exportsHistoryPageQueryOptions(client: ApiClient, workspaceSlug: string, cursor: string) {
  return queryOptions({
    queryKey: [...exportsKeys.history(workspaceSlug), cursor],
    queryFn: () => listWorkspaceExports(client, workspaceSlug, { perPage: EXPORT_HISTORY_PAGE_SIZE, cursor }),
    // Paging steps over stale rows, never a flash of the empty view.
    placeholderData: keepPreviousData,
    retry: false,
    refetchInterval: (query) => historyQuery(query.state.data?.results ?? []).refetchInterval,
  });
}

/** One history window; the section reads through this hook. */
export function useExportsHistoryPage(workspaceSlug: string, cursor: string = EXPORT_HISTORY_FIRST_CURSOR) {
  return useQuery(exportsHistoryPageQueryOptions(getAppClient(), workspaceSlug, cursor));
}
