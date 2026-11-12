// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle shell reads (parity/specs/cycles.md §18 and §23, CYC-012,
// CYC-045). The live list renders its previous data while a background
// refetch runs (the screen keys its skeleton on the pending state, never
// on fetching), so a refresh never flashes the skeleton or the zero
// state. The archived list pins refetchOnWindowFocus off: returning to
// the tab must not reorder or flash the archived rows. Detail, hero,
// progress and analytics reads belong to the slices that render them;
// their keys already live in keys.ts.

import { queryOptions } from "@tanstack/react-query";
import { listArchivedCycles, listCycleRows, type ApiClient } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { cycleKeys } from "./keys.js";

/** The project's live cycles; the server answers a bare array (§23). */
export function cyclesListQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: cycleKeys.list(workspaceSlug, projectId),
    queryFn: () => listCycleRows(client, workspaceSlug, projectId),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** The project's archived cycles; focus never refetches it (CYC-045). */
export function archivedCyclesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: cycleKeys.archived(workspaceSlug, projectId),
    queryFn: () => listArchivedCycles(client, workspaceSlug, projectId),
    staleTime: LIST_STALE_TIME_MS,
    refetchOnWindowFocus: false,
  });
}
