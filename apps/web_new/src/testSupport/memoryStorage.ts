// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Deterministic profile storage for jsdom suites. Node 26 ships a global
// localStorage that is undefined without --localstorage-file and shadows
// the one jsdom would install; with the flag it persists across test files
// in one process, so suites leak state into each other. Each jsdom test
// file instead gets a fresh in-memory Storage that tests may stub like any
// browser profile.

/** Build a Storage-compatible map that can be installed on a global. */
export function createMemoryStorage(): Storage {
  const entries = new Map<string, string>();
  return {
    get length() {
      return entries.size;
    },
    clear: () => {
      entries.clear();
    },
    getItem: (key: string) => entries.get(key) ?? null,
    key: (index: number) => [...entries.keys()][index] ?? null,
    removeItem: (key: string) => {
      entries.delete(key);
    },
    setItem: (key: string, value: string) => {
      entries.set(key, String(value));
    },
  };
}

/** Install the storage when a DOM exists; Node-env suites get nothing. */
export function installTestStorage(scope: { document?: unknown; localStorage?: unknown }): void {
  if (scope.document === undefined) return;
  const storage = createMemoryStorage();
  Object.defineProperty(scope, "localStorage", { value: storage, configurable: true, writable: true });
}
