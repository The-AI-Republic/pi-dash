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
import { pageMayHaveChildren, pageParentId } from "../lib/pageNested.js";
import { pageKeys } from "./keys.js";

export function pageChildrenQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  includeArchived = false
) {
  return queryOptions({
    queryKey: pageKeys.children(workspaceSlug, projectId, pageId, includeArchived),
    queryFn: () => listPages(client, workspaceSlug, projectId, { parent: pageId, includeArchived }),
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

export interface PageForestReadOptions {
  /** Reuse an already-loaded top-level collection instead of reading it. */
  roots?: readonly PageRow[] | undefined;
  /**
   * Probe every row, including ones reporting no live children. Discovery
   * surfaces need this because `sub_pages_count` hides archived children
   * (§11.7); a count-driven walk is enough where archived rows can never be
   * a result (the move picker).
   */
  deep?: boolean | undefined;
}

/**
 * Walk the whole visible tree: the top-level collection, then the children
 * of every page, breadth-first, until nothing is left.
 *
 * Search, filters and favourites are client-side dimensions today, so a
 * discovery pass that must also find child pages needs the whole forest in
 * hand; the tree view keeps using the lazy per-parent read. Rows only
 * arrive here if the caller can see them, so the walk naturally excludes
 * invisible pages and their counts.
 *
 * Every row is probed, not only the ones with a positive
 * `sub_pages_count`: that field counts *visible, non-archived* children
 * (§11.7), so a live parent with a shelved child reports zero and a
 * count-driven walk would never reach it (PAGE-088/089/090). Discovery is
 * the only surface that pays this cost; `roots` lets a caller that already
 * holds the collection (the list, which caches it) reuse that read instead
 * of issuing a second top-level one (PAGE-020).
 */
export async function readPageForest(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  options: PageForestReadOptions = {}
): Promise<PageForest> {
  const { roots, deep = false } = options;
  const topLevel =
    roots === undefined ? await listPages(client, workspaceSlug, projectId, { includeArchived: true }) : roots;
  const rows: PageRow[] = [...topLevel];
  const seen = new Set(rows.map((row) => row.id));
  const frontier = deep ? [...rows] : rows.filter((row) => pageMayHaveChildren(row));
  let complete = true;
  while (frontier.length > 0) {
    if (rows.length >= PAGE_FOREST_LIMIT) {
      complete = false;
      break;
    }
    const batch = frontier.splice(0, frontier.length);
    const childLists = await Promise.all(
      batch.map((parent) => listPages(client, workspaceSlug, projectId, { parent: parent.id, includeArchived: true }))
    );
    for (const children of childLists) {
      for (const child of children) {
        if (seen.has(child.id)) continue;
        seen.add(child.id);
        rows.push(child);
        // A child read can reveal an archived descendant under a live
        // parent, so a deep walk probes every row it discovers; the shallow
        // walk only follows rows that report live children.
        if (deep || pageMayHaveChildren(child)) frontier.push(child);
      }
    }
  }
  return { rows, complete };
}

/**
 * Cache key for one project's whole visible forest. The deep walk sees more
 * rows than the count-driven one, so the two never share an entry.
 */
export function pageForestKey(workspaceSlug: string, projectId: string, deep = false) {
  return [...pageKeys.all(workspaceSlug), "forest", projectId, deep ? "deep" : "shallow"] as const;
}

export function pageForestQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  enabled: boolean,
  options: PageForestReadOptions = {}
) {
  return queryOptions({
    queryKey: pageForestKey(workspaceSlug, projectId, options.deep === true),
    queryFn: () => readPageForest(client, workspaceSlug, projectId, options),
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
