// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Query cache persistence (Data layer page). The desktop build persists
// the full cache and renders from it on launch; the web build persists
// reference data only. Payloads carry a version buster: a mismatch
// discards the stored cache instead of hydrating stale shapes.

import { dehydrate, hydrate, type DehydratedState, type QueryClient } from "@tanstack/react-query";

import type { KeyValueStore } from "../platform/types.js";

/** Bump when a stored cache must no longer hydrate (schema/shape change). */
export const QUERY_CACHE_VERSION = 1;

export const QUERY_CACHE_STORAGE_KEY = "tanstack-query-cache";

/**
 * Reference queries carry the "reference" segment in their key, e.g.
 * ["ws", slug, "reference", "states"]. Feature key factories follow this
 * convention so the web build can persist exactly this subset.
 */
export function isReferenceQueryKey(queryKey: readonly unknown[]): boolean {
  return queryKey.includes("reference");
}

/** Desktop persists everything; web persists reference data only. */
export function persistEverything(): boolean {
  return true;
}

export function persistReferenceOnly(queryKey: readonly unknown[]): boolean {
  return isReferenceQueryKey(queryKey);
}

export interface PersistorOptions {
  version?: number;
  storageKey?: string;
  shouldPersist?: (queryKey: readonly unknown[]) => boolean;
  /** Debounce between a cache change and the write. */
  saveDelayMs?: number;
}

interface StoredPayload {
  version: number;
  state: DehydratedState;
}

export interface CachePersistor {
  save(): Promise<void>;
  restore(): Promise<void>;
  stop(): void;
}

/**
 * A settled query is past its first read. Pending entries cannot be
 * restored from JSON (their in-flight promise loses its methods), and
 * restoring one would abort the rest of the payload. Error entries are
 * settled on purpose: a failed refetch keeps the previously fetched data,
 * which is exactly what a warm start paints.
 */
function settledStatus(status: unknown): boolean {
  return status === "success" || status === "error";
}

function restorableQuery(entry: DehydratedState["queries"][number]): boolean {
  return settledStatus(entry.state?.status) && entry.promise === undefined;
}

export function createCachePersistor(
  client: QueryClient,
  storage: KeyValueStore,
  options: PersistorOptions = {}
): CachePersistor {
  const version = options.version ?? QUERY_CACHE_VERSION;
  const storageKey = options.storageKey ?? QUERY_CACHE_STORAGE_KEY;
  const shouldPersist = options.shouldPersist ?? persistEverything;
  const saveDelayMs = options.saveDelayMs ?? 1000;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let stopped = false;

  function scheduleSave(): void {
    if (stopped) return;
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = undefined;
      void save();
    }, saveDelayMs);
    if (typeof timer === "object" && typeof (timer as { unref?: () => void }).unref === "function") {
      (timer as unknown as { unref: () => void }).unref?.();
    }
  }

  async function save(): Promise<void> {
    // Only settled data can be painted on launch. A pending query has no
    // rows yet, and `dehydrate` would attach its in-flight promise, which
    // JSON turns into `{}` and which then breaks the whole restore below.
    const full = dehydrate(client, {
      shouldDehydrateQuery: (query) => settledStatus(query.state.status) && shouldPersist(query.queryKey),
    });
    const payload: StoredPayload = { version, state: full };
    try {
      await storage.set(storageKey, JSON.stringify(payload));
    } catch {
      // Persistence is best-effort; the cache simply rebuilds from network.
    }
  }

  async function restore(): Promise<void> {
    let raw: string | null;
    try {
      raw = await storage.get(storageKey);
    } catch {
      return;
    }
    if (!raw) return;
    let payload: StoredPayload;
    try {
      payload = JSON.parse(raw) as StoredPayload;
    } catch {
      await storage.remove(storageKey).catch(() => undefined);
      return;
    }
    if (payload.version !== version || !payload.state) {
      await storage.remove(storageKey).catch(() => undefined);
      return;
    }
    // A payload written before the save-side guard can still carry pending
    // entries whose promise did not survive JSON. `hydrate` resolves those
    // promises synchronously and throws, which abandons every remaining
    // entry, so drop them here and restore the rest.
    const state: DehydratedState = {
      ...payload.state,
      queries: (payload.state.queries ?? []).filter(restorableQuery),
    };
    hydrate(client, state);
  }

  const unsubscribe = client.getQueryCache().subscribe(scheduleSave);

  return {
    save,
    restore,
    stop: () => {
      stopped = true;
      if (timer !== undefined) clearTimeout(timer);
      unsubscribe();
    },
  };
}
