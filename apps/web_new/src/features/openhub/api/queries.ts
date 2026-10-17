// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Apps catalog reads (written from parity/specs/cloud.md §5,
// CLOUD-022/023): the status answer gating the catalog, and one
// marketplace window per settled query plus cursor. Both stay disabled
// without a workspace so no request fires.
import { keepPreviousData, queryOptions, useQuery } from "@tanstack/react-query";
import { getOpenhubStatus, listMarketplaceApps, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { openhubKeys } from "./keys.js";

/** Catalog window size (CLOUD-023: the oracle pins limit=24 on every read). */
export const APPS_CATALOG_PAGE_SIZE = 24;

/** Idle pause before a keystroke burst filters the catalog (CLOUD-023: a few hundred milliseconds). */
export const APPS_SEARCH_DEBOUNCE_MS = 300;

export function openhubStatusQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: openhubKeys.status(workspaceSlug),
    queryFn: () => getOpenhubStatus(client),
    staleTime: LIST_STALE_TIME_MS,
    // Every failure surfaces with a manual retry (CLOUD-030); automatic
    // retries would swallow the fail-once-then-live shapes the oracle
    // pins, and the transport-failure panel NEWFRONT-473 targets.
    retry: false,
    enabled: workspaceSlug.length > 0,
  });
}

/** Catalog gate for one workspace; components read through this hook. */
export function useOpenhubStatus(workspaceSlug: string) {
  return useQuery(openhubStatusQueryOptions(getAppClient(), workspaceSlug));
}

export interface MarketplacePageParams {
  /** Settled, trimmed query text; blank reads the unfiltered window. */
  query: string;
  /** Window cursor; null reads the first window. */
  cursor: string | null;
}

export function marketplacePageQueryOptions(client: ApiClient, workspaceSlug: string, params: MarketplacePageParams) {
  return queryOptions({
    queryKey: openhubKeys.marketplacePage(workspaceSlug, params.query, params.cursor),
    queryFn: () =>
      listMarketplaceApps(client, {
        // Absent keys read as undefined at the contract, so the initial
        // read carries only the page size (exactOptionalPropertyTypes).
        ...(params.query === "" ? {} : { q: params.query }),
        ...(params.cursor === null ? {} : { cursor: params.cursor }),
        limit: APPS_CATALOG_PAGE_SIZE,
      }),
    staleTime: LIST_STALE_TIME_MS,
    // Paging and searching step over stale content, never a flash of empty.
    placeholderData: keepPreviousData,
    // Same as the status read: failures surface inline with their retry.
    retry: false,
    enabled: workspaceSlug.length > 0,
  });
}

/** One catalog window; components read through this hook. */
export function useMarketplacePage(workspaceSlug: string, params: MarketplacePageParams) {
  return useQuery(marketplacePageQueryOptions(getAppClient(), workspaceSlug, params));
}
