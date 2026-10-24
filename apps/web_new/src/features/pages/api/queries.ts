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
    queryFn: () => listPages(client, workspaceSlug, projectId),
    staleTime: LIST_STALE_TIME_MS,
  });
}
