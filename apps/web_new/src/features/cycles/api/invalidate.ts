// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle freshness (parity/specs/cycles.md §18, CYC-045). Mutations
// refresh only the reads they affect — the project lists, the touched
// cycles' own reads, or both sides of a transfer — never the whole
// area. Later slices call these from their mutation settlements; the
// archived list stays out of every automatic refetch (see queries.ts).

import type { QueryClient } from "@tanstack/react-query";
import { CYCLE_ANALYTICS_TYPES } from "@pidash/api-client";

import { cycleKeys } from "./keys.js";

/** Live list reads of one project: the list and the hero's current read. */
export function invalidateCycleLists(
  queryClient: QueryClient,
  workspaceSlug: string,
  projectId: string
): Promise<void> {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: cycleKeys.list(workspaceSlug, projectId) }),
    queryClient.invalidateQueries({ queryKey: cycleKeys.hero(workspaceSlug, projectId) }),
  ]).then(() => undefined);
}

/**
 * One cycle's own reads: the detail/peek row, the progress snapshot and
 * series, and every analytics breakdown. Favoriting and edits settle
 * through here so the row and the sidebar agree.
 */
export function invalidateCycleReads(
  queryClient: QueryClient,
  workspaceSlug: string,
  projectId: string,
  cycleId: string
): Promise<void> {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: cycleKeys.detail(workspaceSlug, projectId, cycleId) }),
    queryClient.invalidateQueries({ queryKey: cycleKeys.progress(workspaceSlug, projectId, cycleId) }),
    queryClient.invalidateQueries({
      queryKey: cycleKeys.detailedProgress(workspaceSlug, projectId, cycleId),
    }),
    ...CYCLE_ANALYTICS_TYPES.map((type) =>
      queryClient.invalidateQueries({ queryKey: cycleKeys.analytics(workspaceSlug, projectId, cycleId, type) })
    ),
    queryClient.invalidateQueries({ queryKey: cycleKeys.heroIssues(workspaceSlug, projectId, cycleId) }),
  ]).then(() => undefined);
}

/**
 * Archived reads of one project: the archived list and every archived
 * peek nested under it. Archive and restore settle through here; window
 * focus never refetches them (see queries.ts).
 */
export function invalidateArchivedCycleReads(
  queryClient: QueryClient,
  workspaceSlug: string,
  projectId: string
): Promise<void> {
  return queryClient.invalidateQueries({ queryKey: cycleKeys.archived(workspaceSlug, projectId) });
}

/**
 * After a create, edit, delete, archive, restore or favorite: the
 * project lists plus the touched cycle's own reads. Creation has no
 * cycle id yet, so it refreshes the lists only.
 */
export function invalidateAfterCycleWrite(
  queryClient: QueryClient,
  workspaceSlug: string,
  projectId: string,
  cycleId: string | null
): Promise<void> {
  const lists = invalidateCycleLists(queryClient, workspaceSlug, projectId);
  if (cycleId === null) return lists;
  return Promise.all([
    lists,
    invalidateCycleReads(queryClient, workspaceSlug, projectId, cycleId),
    invalidateArchivedCycleReads(queryClient, workspaceSlug, projectId),
  ]).then(() => undefined);
}

/**
 * After a transfer: both sides refresh — the source's banner, counts
 * and ring as well as the target (CYC-045, NEWFRONT-267 intended) —
 * plus the project lists both rows render in.
 */
export function invalidateAfterCycleTransfer(
  queryClient: QueryClient,
  workspaceSlug: string,
  projectId: string,
  sourceCycleId: string,
  targetCycleId: string
): Promise<void> {
  return Promise.all([
    invalidateCycleLists(queryClient, workspaceSlug, projectId),
    invalidateCycleReads(queryClient, workspaceSlug, projectId, sourceCycleId),
    invalidateCycleReads(queryClient, workspaceSlug, projectId, targetCycleId),
  ]).then(() => undefined);
}
