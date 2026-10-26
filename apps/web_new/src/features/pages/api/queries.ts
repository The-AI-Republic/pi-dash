// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages collection read (parity/specs/pages.md §3.9 and §11.1,
// PAGE-020). One request returns the project's full top-level collection;
// tabs, search, sort and filters are client-side and never refetch. The
// server's own ordering is not shown by the list: the screen always applies
// the default modification-descending sort.

import { queryOptions } from "@tanstack/react-query";
import { listPages, type ApiClient } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { pageKeys } from "./keys.js";

export function pagesListQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: pageKeys.list(workspaceSlug, projectId),
    // The one collection serves every tab, including Archived, so it asks
    // for shelved rows too; the tab partition is still client-side.
    queryFn: () => listPages(client, workspaceSlug, projectId, { includeArchived: true }),
    staleTime: LIST_STALE_TIME_MS,
    // Returning to the list (back from a page, a project switch back) is a
    // remount, not a fresh session: the spec asks for one revalidation
    // over the held rows (§3.9, PAGE-020). Only a mount re-runs it, so tab
    // switches, search and sort stay purely client-side.
    refetchOnMount: "always",
  });
}
