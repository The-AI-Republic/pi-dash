// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Fetch-on-mount reads (parity/specs/issues.md §18, ISS-221; reused by
// parity/specs/modules.md §8.5, MOD-039). A read built with this spread
// loads when it first gains a subscriber, then stays as it is: a window
// focus, a network reconnect or a timer never refetches it, so another
// member's write appears only after navigation or a reload. The acting
// user's own mutations still refresh their views, because those settle
// through an explicit invalidation rather than a background refetch.
//
// The shared client turns focus and reconnect refetch off by default, but
// a default is only a fallback: any option a caller does not state can be
// overridden for its subtree. Stating all three here keeps the guarantee
// local to the read, and the no-live pins read these values back.

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";

/** The three refetch triggers a fetch-on-mount read switches off. */
export interface FetchOnMountFreshness {
  staleTime: number;
  refetchOnWindowFocus: false;
  refetchOnReconnect: false;
  refetchInterval: false;
}

/**
 * Freshness for a read that may only load on mount and on the owner's
 * own writes. `staleTime` keeps the mount quiet while the data is young;
 * the three `false`s keep every background trigger quiet forever.
 */
export const FETCH_ON_MOUNT_FRESHNESS: FetchOnMountFreshness = Object.freeze({
  staleTime: LIST_STALE_TIME_MS,
  refetchOnWindowFocus: false,
  refetchOnReconnect: false,
  refetchInterval: false,
});

/**
 * The option block for one fetch-on-mount read, with the caller's own
 * extras kept exactly as typed. Wrap the object literal inside
 * `queryOptions(fetchOnMountOptions({ queryKey, queryFn }))` so the read
 * cannot accidentally leave a trigger on, and the outer helper still
 * infers the data and error types from the caller.
 */
export function fetchOnMountOptions<T extends object>(read: T): T & FetchOnMountFreshness {
  return { ...read, ...FETCH_ON_MOUNT_FRESHNESS };
}
