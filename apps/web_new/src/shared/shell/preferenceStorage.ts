// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sync preference storage for persisted UI state (SHELL-063, SHELL-066,
// from parity/specs/shell.md §5). Density, docking, collapse and peek
// survive reload through the browser profile; private modes and missing
// DOM storage fall back to a process-local map so the stores never throw.
// Keys are namespaced pidash:* so the old and new apps never share them
// while both run on one origin during migration. Server-backed
// preferences live in ./preferences.js; this module stays dependency-free
// so the initial shell chain can import it without dragging mutations in.
import type { StateStorage } from "zustand/middleware";

let memoryFallback: StateStorage | null = null;

function memoryStorage(): StateStorage {
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
export function preferenceStorage(): StateStorage {
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
