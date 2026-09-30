// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
    const full = dehydrate(client, {
      shouldDehydrateQuery: (query) => shouldPersist(query.queryKey),
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
    hydrate(client, payload.state);
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
