// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Nested tree reads (parity/specs/pages.md §11.7, PAGE-085). The default
// collection read stays top-level only, so each expanded parent asks for
// its own children with `?parent=<id>`. Every child list is cached under
// its parent, and a settled write that changes the tree invalidates the
// whole pages subtree so counts and children move together.

import { queryOptions } from "@tanstack/react-query";
import { listPages, type ApiClient, type PageRow } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { pageChildCount, pageParentId } from "../lib/pageNested.js";
import { pageKeys } from "./keys.js";

export function pageChildrenQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, pageId: string) {
  return queryOptions({
    queryKey: pageKeys.children(workspaceSlug, projectId, pageId),
    queryFn: () => listPages(client, workspaceSlug, projectId, { parent: pageId }),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** Hard ceiling on one forest walk, so a malformed tree cannot spin. */
export const PAGE_FOREST_LIMIT = 500;

export interface PageForest {
  rows: PageRow[];
  /** False when the walk stopped at `PAGE_FOREST_LIMIT`. */
  complete: boolean;
}

/**
 * Walk the whole visible tree: the top-level collection, then the children
 * of every page that reports any, breadth-first, until nothing is left.
 *
 * Search, filters and favourites are client-side dimensions today, so a
 * discovery pass that must also find child pages needs the whole forest in
 * hand; the tree view keeps using the lazy per-parent read. Rows only
 * arrive here if the caller can see them, so the walk naturally excludes
 * invisible pages and their counts.
 */
export async function readPageForest(client: ApiClient, workspaceSlug: string, projectId: string): Promise<PageForest> {
  const roots = await listPages(client, workspaceSlug, projectId);
  const rows: PageRow[] = [...roots];
  const seen = new Set(rows.map((row) => row.id));
  const frontier = roots.filter((row) => pageChildCount(row) > 0);
  let complete = true;
  while (frontier.length > 0) {
    if (rows.length >= PAGE_FOREST_LIMIT) {
      complete = false;
      break;
    }
    const batch = frontier.splice(0, frontier.length);
    const childLists = await Promise.all(
      batch.map((parent) => listPages(client, workspaceSlug, projectId, { parent: parent.id }))
    );
    for (const children of childLists) {
      for (const child of children) {
        if (seen.has(child.id)) continue;
        seen.add(child.id);
        rows.push(child);
        if (pageChildCount(child) > 0) frontier.push(child);
      }
    }
  }
  return { rows, complete };
}

/** Cache key for one project's whole visible forest. */
export function pageForestKey(workspaceSlug: string, projectId: string) {
  return [...pageKeys.all(workspaceSlug), "forest", projectId] as const;
}

export function pageForestQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, enabled: boolean) {
  return queryOptions({
    queryKey: pageForestKey(workspaceSlug, projectId),
    queryFn: () => readPageForest(client, workspaceSlug, projectId),
    staleTime: LIST_STALE_TIME_MS,
    enabled,
  });
}

/** Rows whose parent is not in the pool are roots for display purposes. */
export function rootsOf(rows: readonly PageRow[]): PageRow[] {
  const present = new Set(rows.map((row) => row.id));
  return rows.filter((row) => {
    const parentId = pageParentId(row);
    return parentId === null || !present.has(parentId);
  });
}
