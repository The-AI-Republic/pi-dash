// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Optimistic-mutation helpers (Data layer page). Field edits, state moves,
// reorders and comments patch the cache in onMutate, roll back in onError,
// and invalidate in onSettled. Features compose these primitives; the
// toast on rollback stays in the feature mutation.

import type { QueryClient, QueryKey } from "@tanstack/react-query";

export type Rollback = () => void;

/** Previous data for every query matching the key, for later rollback. */
export interface QuerySnapshot {
  queryKey: QueryKey;
  data: unknown;
}

export function snapshotQueries(client: QueryClient, queryKey: QueryKey): QuerySnapshot[] {
  return client
    .getQueryCache()
    .findAll({ queryKey })
    .map((query) => ({
      queryKey: query.queryKey,
      data: query.state.data,
    }));
}

function restoreSnapshot(client: QueryClient, snapshot: QuerySnapshot[]): void {
  for (const entry of snapshot) {
    client.setQueryData(entry.queryKey, entry.data);
  }
}

/**
 * Patch one cache entry optimistically. Returns a rollback that restores
 * the previous value. Read the snapshot before the first patch when a
 * mutation touches several keys, then roll back all of them on error.
 */
export function applyOptimisticUpdate<T>(
  client: QueryClient,
  queryKey: QueryKey,
  updater: (previous: T | undefined) => T | undefined
): Rollback {
  const previous = client.getQueryData<T>(queryKey);
  client.setQueryData<T | undefined>(queryKey, updater(previous));
  let done = false;
  return () => {
    if (done) return;
    done = true;
    client.setQueryData(queryKey, previous);
  };
}

/** Roll back every entry captured by snapshotQueries. */
export function rollbackSnapshots(client: QueryClient, snapshots: QuerySnapshot[]): void {
  restoreSnapshot(client, snapshots);
}

/** Mark a whole workspace subtree stale after a mutation settles. */
export function invalidateWorkspace(client: QueryClient, workspaceSlug: string): Promise<void> {
  return client.invalidateQueries({ queryKey: ["ws", workspaceSlug] });
}

/** Drop a whole workspace subtree, e.g. when leaving the workspace. */
export function removeWorkspace(client: QueryClient, workspaceSlug: string): void {
  client.removeQueries({ queryKey: ["ws", workspaceSlug] });
}
