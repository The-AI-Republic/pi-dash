// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).

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
