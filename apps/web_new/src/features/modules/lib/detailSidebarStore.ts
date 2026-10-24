// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail sidebar collapse state (parity/specs/modules.md §8.4,
// MOD-032). Personal display state: one boolean in browser storage,
// restored on reload. The header toggle and the sidebar's own close
// control write through the same setter, so both paths persist.

import * as React from "react";

import type { StorageLike } from "./listLayoutStore.js";

/** Browser key holding the detail sidebar's collapsed flag. */
export const MODULE_DETAIL_SIDEBAR_STORAGE_KEY = "module_detail_sidebar_collapsed";

function defaultStorage(): StorageLike | undefined {
  try {
    return globalThis.localStorage;
  } catch {
    return undefined;
  }
}

/** Parse the stored flag; absent or corrupt reads as expanded. */
export function parseSidebarCollapsed(raw: string | null): boolean {
  if (raw === null) return false;
  try {
    return JSON.parse(raw) === true;
  } catch {
    return raw === "true";
  }
}

/** The stored collapsed flag (false for an absent or corrupt entry). */
export function readSidebarCollapsed(storage: StorageLike | undefined = defaultStorage()): boolean {
  if (storage === undefined) return false;
  try {
    return parseSidebarCollapsed(storage.getItem(MODULE_DETAIL_SIDEBAR_STORAGE_KEY));
  } catch {
    return false;
  }
}

/** Persist the collapsed flag. */
export function writeSidebarCollapsed(collapsed: boolean, storage: StorageLike | undefined = defaultStorage()): void {
  if (storage === undefined) return;
  try {
    storage.setItem(MODULE_DETAIL_SIDEBAR_STORAGE_KEY, JSON.stringify(collapsed));
  } catch {
    // A browser that refuses writes keeps the in-memory choice for this
    // visit; the next reload falls back to expanded.
  }
}

export interface DetailSidebarHandle {
  collapsed: boolean;
  setCollapsed: (collapsed: boolean) => void;
}

/** The sidebar's collapsed flag plus its persisting setter. */
export function useDetailSidebarCollapsed(): DetailSidebarHandle {
  const [collapsed, setCollapsedState] = React.useState<boolean>(() => readSidebarCollapsed());

  const setCollapsed = React.useCallback((next: boolean) => {
    writeSidebarCollapsed(next);
    setCollapsedState(next);
  }, []);

  return { collapsed, setCollapsed };
}
