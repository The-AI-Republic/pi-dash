// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Backing stores for the Platform key/value surface. web.ts and tauri.ts
// pick a namespace; the query persistence layer only sees KeyValueStore.

import type { KeyValueStore } from "./types.js";

/** Ephemeral store. Used in tests and when no durable store is available. */
export function createMemoryStore(): KeyValueStore {
  const entries = new Map<string, string>();
  return {
    get: (key) => Promise.resolve(entries.get(key) ?? null),
    set: (key, value) => {
      entries.set(key, value);
      return Promise.resolve();
    },
    remove: (key) => {
      entries.delete(key);
      return Promise.resolve();
    },
    clear: () => {
      entries.clear();
      return Promise.resolve();
    },
  };
}

function namespacedKey(namespace: string, key: string): string {
  return `${namespace}:${key}`;
}

function domStorage(): Storage | undefined {
  if (typeof globalThis.window === "undefined") return undefined;
  try {
    const storage = globalThis.window.localStorage;
    const probe = "__pidash_probe__";
    storage.setItem(probe, "1");
    storage.removeItem(probe);
    return storage;
  } catch {
    return undefined;
  }
}

/** Browser profile store with an in-memory fallback for private modes. */
export function createLocalStorageStore(namespace: string): KeyValueStore {
  const backing = domStorage();
  const memory = createMemoryStore();
  if (!backing) return memory;
  return {
    get: (key) => {
      try {
        return Promise.resolve(backing.getItem(namespacedKey(namespace, key)));
      } catch {
        return memory.get(key);
      }
    },
    set: (key, value) => {
      try {
        backing.setItem(namespacedKey(namespace, key), value);
        return Promise.resolve();
      } catch {
        return memory.set(key, value);
      }
    },
    remove: (key) => {
      try {
        backing.removeItem(namespacedKey(namespace, key));
        return Promise.resolve();
      } catch {
        return memory.remove(key);
      }
    },
    clear: () => {
      try {
        const doomed: string[] = [];
        for (let index = 0; index < backing.length; index += 1) {
          const found = backing.key(index);
          if (found?.startsWith(`${namespace}:`)) doomed.push(found);
        }
        for (const found of doomed) backing.removeItem(found);
        return Promise.resolve();
      } catch {
        return memory.clear();
      }
    },
  };
}

const INDEXED_DB_NAME = "pidash-web-new";
const INDEXED_DB_STORE = "keyvalue";

function openDatabase(dbName: string): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = globalThis.indexedDB.open(dbName, 1);
    request.onupgradeneeded = () => {
      if (!request.result.objectStoreNames.contains(INDEXED_DB_STORE)) {
        request.result.createObjectStore(INDEXED_DB_STORE);
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error("IndexedDB open failed"));
  });
}

function indexedDbAvailable(): boolean {
  return typeof globalThis.indexedDB !== "undefined";
}

/**
 * Larger-quota store for the query cache. Falls back to the profile store,
 * then memory, so callers never branch on availability.
 */
export function createIndexedDbStore(namespace: string): KeyValueStore {
  const fallback = createLocalStorageStore(namespace);
  if (!indexedDbAvailable()) return fallback;
  let database: Promise<IDBDatabase> | undefined;
  const db = () => (database ??= openDatabase(INDEXED_DB_NAME));

  const run = <T>(mode: IDBTransactionMode, task: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> =>
    db().then(
      (opened) =>
        new Promise<T>((resolve, reject) => {
          const tx = opened.transaction(INDEXED_DB_STORE, mode);
          const outcome = task(tx.objectStore(INDEXED_DB_STORE));
          outcome.onsuccess = () => resolve(outcome.result);
          outcome.onerror = () => reject(outcome.error ?? new Error("IndexedDB request failed"));
        })
    );

  const withFallback = <T>(task: () => Promise<T>, local: () => Promise<T>): Promise<T> => task().catch(() => local());

  return {
    get: (key) =>
      withFallback(
        () =>
          run("readonly", (store) => store.get(namespacedKey(namespace, key))).then((value) =>
            typeof value === "string" ? value : null
          ),
        () => fallback.get(key)
      ),
    set: (key, value) =>
      withFallback(
        () => run("readwrite", (store) => store.put(value, namespacedKey(namespace, key))).then(() => undefined),
        () => fallback.set(key, value)
      ),
    remove: (key) =>
      withFallback(
        () => run("readwrite", (store) => store.delete(namespacedKey(namespace, key))).then(() => undefined),
        () => fallback.remove(key)
      ),
    clear: () => fallback.clear(),
  };
}

/**
 * Default durable store for a platform implementation: the larger-quota
 * store first, degrading silently to profile storage, then memory.
 */
export function createPersistentStore(namespace: string): KeyValueStore {
  return createIndexedDbStore(namespace);
}
