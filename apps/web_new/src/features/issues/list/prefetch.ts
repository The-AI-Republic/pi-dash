// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared first-page prefetch for the cycle and module list hosts
// (parity/specs/issues.md §2, ISS-007–014). The route definitions live in
// the eager route graph, so the prefetch body rides this module's chunk
// instead of being duplicated into the entry bundle (Quality gates).

import { type QueryClient } from "@tanstack/react-query";
import type { ApiClient } from "@pidash/api-client";

import { selectDisplayPrefs } from "../prefs/displayPrefs.js";
import { prefsQueryOptions } from "../prefs/prefsQuery.js";
import type { IssuesEntity } from "../prefs/entities.js";
import { loadListLayout } from "./lazy.js";
import { listPageQueryOptions } from "./pages.js";

export interface ListPrefetchContext {
  queryClient: QueryClient;
  apiClient: ApiClient;
}

/**
 * Read the entity's display preferences, then start the list layout chunk
 * and the first page together. The stored sub-issue flag decides whether
 * child rows belong in the first page.
 */
export async function prefetchEntityList(
  context: ListPrefetchContext,
  workspaceSlug: string,
  entity: IssuesEntity
): Promise<void> {
  const record = await context.queryClient.ensureQueryData(prefsQueryOptions(context.apiClient, workspaceSlug, entity));
  const showSubIssues = selectDisplayPrefs(record).showSubIssues;
  await Promise.all([
    loadListLayout(),
    context.queryClient.ensureQueryData(
      listPageQueryOptions(context.apiClient, workspaceSlug, {
        entity,
        groupBy: null,
        orderBy: "sort_order",
        showSubIssues,
      })
    ),
  ]);
}
