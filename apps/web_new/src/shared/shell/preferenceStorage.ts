// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sync preference storage for persisted UI state (SHELL-063, SHELL-066,
// from parity/specs/shell.md §5). Density, docking, collapse and peek
// survive reload through the browser profile; private modes and missing
// DOM storage fall back to a process-local map so the stores never throw.
// Keys are namespaced pidash:* so the old and new apps never share them
// while both run on one origin during migration. Persistence is a tiny
// load/store pair instead of the zustand middleware: these stores are the
// only persisted state in the initial-JS chain, and the middleware would
// ride along (size budget). Corrupt or foreign payloads fall back to the
// caller defaults instead of throwing. Server-backed preferences live in
// ./preferences.js; this module stays dependency-free so the initial shell
// chain can import it without dragging mutations in.

/** The storage surface preferences need: a subset of web Storage. */
export interface PreferenceStorage {
  getItem: (key: string) => string | null;
  setItem: (key: string, value: string) => void;
  removeItem: (key: string) => void;
}

let memoryFallback: PreferenceStorage | null = null;

function memoryStorage(): PreferenceStorage {
  memoryFallback ??= (() => {
    const entries = new Map<string, string>();
    return {
      getItem: (key: string) => entries.get(key) ?? null,
      setItem: (key: string, value: string) => {
        entries.set(key, value);
      },
      removeItem: (key: string) => {
        entries.delete(key);
      },
    };
  })();
  return memoryFallback;
}

/** Browser profile storage when writable, otherwise the memory fallback. */
export function preferenceStorage(): PreferenceStorage {
  try {
    const backing = globalThis.localStorage;
    const probe = "__pidash_preference_probe__";
    backing.setItem(probe, "1");
    backing.removeItem(probe);
    return backing;
  } catch {
    return memoryStorage();
  }
}

/**
 * Read a JSON preference. Missing keys, unreadable storage, invalid JSON
 * and payloads failing validation all resolve to the fallback.
 */
export function loadPreference<T>(key: string, fallback: T, isValid: (value: unknown) => value is T): T {
  try {
    const raw = preferenceStorage().getItem(key);
    if (raw === null) return fallback;
    const parsed: unknown = JSON.parse(raw);
    return isValid(parsed) ? parsed : fallback;
  } catch {
    return fallback;
  }
}

/** Write a JSON preference; storage failures are swallowed by design. */
export function storePreference(key: string, value: unknown): void {
  try {
    preferenceStorage().setItem(key, JSON.stringify(value));
  } catch {
    // Private modes and quota errors keep the in-memory state usable.
  }
}

/** Drop a JSON preference; storage failures are swallowed by design. */
export function removePreference(key: string): void {
  try {
    preferenceStorage().removeItem(key);
  } catch {
    // Missing storage leaves nothing to clear.
  }
}
