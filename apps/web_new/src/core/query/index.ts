// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

export { createQueryClient, LIST_STALE_TIME_MS, REFERENCE_STALE_TIME_MS, workspaceScope } from "./client.js";
export type { QuerySnapshot, Rollback } from "./optimistic.js";
export {
  applyOptimisticUpdate,
  invalidateWorkspace,
  removeWorkspace,
  rollbackSnapshots,
  snapshotQueries,
} from "./optimistic.js";
export {
  createCachePersistor,
  isReferenceQueryKey,
  persistEverything,
  persistReferenceOnly,
  QUERY_CACHE_STORAGE_KEY,
  QUERY_CACHE_VERSION,
} from "./persistence.js";
export type { CachePersistor, PersistorOptions } from "./persistence.js";
export { createRealtimeHub } from "./realtime.js";
export type { RealtimeEvent, RealtimeHandler, RealtimeHub } from "./realtime.js";
