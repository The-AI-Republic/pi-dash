// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared first-page prefetch for the cycle and module list hosts
// (parity/specs/issues.md §2/§5, ISS-007–014, ISS-036). The route
// definitions live in the eager route graph, so the prefetch body rides
// this module's chunk instead of being duplicated into the entry bundle
// (Quality gates).

import { type QueryClient } from "@tanstack/react-query";
import type { ApiClient } from "@pidash/api-client";

import { selectDisplayPrefs } from "../prefs/displayPrefs.js";
import { prefsQueryOptions } from "../prefs/prefsQuery.js";
import type { IssuesEntity } from "../prefs/entities.js";
import { loadListLayout } from "./lazy.js";
import { loadKanbanLayout } from "../kanban/lazy.js";
import { listPageQueryOptions } from "./pages.js";

export interface ListPrefetchContext {
  queryClient: QueryClient;
  apiClient: ApiClient;
}

/**
 * Read the entity's display preferences, then start the list and board
 * layout chunks and the first page together. The stored sub-issue flag
 * decides whether child rows belong in the first page, and the stored
 * rich expression scopes it exactly as the screen reads it: a cycle or
 * module keeps its filters across visits, so an unscoped prefetch would
 * cache a page the screen never asks for and cost it a second read
 * (ISS-069, NEWFRONT-981).
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
    loadKanbanLayout(),
    context.queryClient.ensureQueryData(
      listPageQueryOptions(context.apiClient, workspaceSlug, {
        entity,
        groupBy: null,
        orderBy: "sort_order",
        showSubIssues,
        richFilters: record.rich_filters,
      })
    ),
  ]);
}
