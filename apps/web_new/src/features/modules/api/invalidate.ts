// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module freshness (parity/specs/modules.md §13, §16). Writes settle by
// refreshing only the reads they changed: the project's lists, one
// module's own detail work-items and links, or the archived reads. Every
// later slice (C–O) calls these instead of invalidating the whole area
// or hand-writing tuples. Module reads are fetch-on-mount (§8.5, MOD-039):
// no polling and no focus refetch, so only these explicit calls refresh a
// view.

import type { QueryClient } from "@tanstack/react-query";

import { moduleKeys } from "./keys.js";

/** The project's active module list (list, board, timeline all read it). */
export function invalidateModuleLists(
  queryClient: QueryClient,
  workspaceSlug: string,
  projectId: string
): Promise<void> {
  return queryClient.invalidateQueries({ queryKey: moduleKeys.list(workspaceSlug, projectId) });
}

/** The workspace-wide module set used by pickers and the palette. */
export function invalidateWorkspaceModules(queryClient: QueryClient, workspaceSlug: string): Promise<void> {
  return queryClient.invalidateQueries({ queryKey: moduleKeys.workspaceList(workspaceSlug) });
}

/** One module's own reads: the detail/peek row, its links and its work items. */
export function invalidateModuleReads(
  queryClient: QueryClient,
  workspaceSlug: string,
  projectId: string,
  moduleId: string
): Promise<void> {
  return queryClient.invalidateQueries({ queryKey: moduleKeys.detail(workspaceSlug, projectId, moduleId) });
}

/** The project's archived list plus every archived peek nested under it. */
export function invalidateArchivedModuleReads(
  queryClient: QueryClient,
  workspaceSlug: string,
  projectId: string
): Promise<void> {
  return queryClient.invalidateQueries({ queryKey: moduleKeys.archivedList(workspaceSlug, projectId) });
}

/**
 * After a create, edit, delete, archive, restore or favorite: the project
 * lists plus the touched module's own reads. A create has no module id
 * yet, so it refreshes the lists (and the archived list, because a delete
 * removes a row from it) only.
 */
export function invalidateAfterModuleWrite(
  queryClient: QueryClient,
  workspaceSlug: string,
  projectId: string,
  moduleId: string | null
): Promise<void> {
  const lists = Promise.all([
    invalidateModuleLists(queryClient, workspaceSlug, projectId),
    invalidateArchivedModuleReads(queryClient, workspaceSlug, projectId),
  ]);
  if (moduleId === null) return lists.then(() => undefined);
  return Promise.all([lists, invalidateModuleReads(queryClient, workspaceSlug, projectId, moduleId)]).then(
    () => undefined
  );
}
