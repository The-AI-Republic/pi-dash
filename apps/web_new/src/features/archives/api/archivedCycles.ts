// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-cycle reads for the archives tab (parity/specs/archives.md
// §4, ARCH-014, ARCH-016). The peek reads one archived cycle through
// the archived detail endpoint — never the live detail, which excludes
// archived rows. The tab's filters live in a namespaced in-memory slot
// beside the live tab's own values, so neither tab leaks into the
// other, re-entering the tab keeps them for the session, and reloads
// forget them like every other in-memory UI state.

import { queryOptions, useQuery } from "@tanstack/react-query";
import {
  getArchivedCycle,
  type ApiClient,
  type ArchivedCycleDetail,
  type CycleRow as CycleRowData,
} from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../../core/query/index.js";
import { useProjectRow } from "../../auth/index.js";
import { cycleKeys, hasCycleFilters, makeCycleFilter, useCycleFilterStore } from "../../cycles/index.js";

/**
 * The filter/search store slot for one project's archived tab. The
 * live tab keys its values by the bare project id; the archived tab
 * reads and writes this slot instead, so applied filters and the
 * remembered query never cross between the two.
 */
export function archivedCycleStoreSlot(projectId: string): string {
  return `archived/${projectId}`;
}

export function archivedCycleDetailQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  cycleId: string
) {
  return queryOptions<ArchivedCycleDetail>({
    queryKey: cycleKeys.archivedDetail(workspaceSlug, projectId, cycleId),
    queryFn: () => getArchivedCycle(client, workspaceSlug, projectId, cycleId),
    staleTime: REFERENCE_STALE_TIME_MS,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

export function useArchivedCycleDetail(workspaceSlug: string, projectId: string, cycleId: string) {
  return useQuery(archivedCycleDetailQueryOptions(getAppClient(), workspaceSlug, projectId, cycleId));
}

/** The archived tab's filter wiring: predicate, badge state, and clear-all. */
export interface ArchivedCycleListFilter {
  /** Row predicate for the current values and the project timezone. */
  predicate: (cycle: CycleRowData) => boolean;
  /** Whether any dimension narrows the list. */
  active: boolean;
  /** Clear every dimension; the search query survives. */
  clearAll: () => void;
}

/**
 * Read the archived slot's filter set with the timezone its date
 * windows resolve in. The predicate is memoized on both, so the list
 * only re-narrows when values change or the project read lands.
 */
export function useArchivedCycleListFilter(workspaceSlug: string, projectId: string): ArchivedCycleListFilter {
  const slot = archivedCycleStoreSlot(projectId);
  const filters = useCycleFilterStore((state) => state.filtersFor(slot));
  const project = useProjectRow(workspaceSlug, projectId);
  const timeZone = project.data?.timezone ?? "UTC";
  const predicate = React.useMemo(() => makeCycleFilter(filters, timeZone), [filters, timeZone]);
  const clearAll = React.useCallback(() => {
    useCycleFilterStore.getState().clearAll(slot);
  }, [slot]);
  return { predicate, active: hasCycleFilters(filters), clearAll };
}
