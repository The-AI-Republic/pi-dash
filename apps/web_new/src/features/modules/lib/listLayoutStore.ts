// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-project modules-list layout (parity/specs/modules.md §3.4 and
// §13.3.1, MOD-011). Personal display state: one entry per project in the
// modules feature's own browser record, restored on reload, never written
// to the project row, the per-module user-properties record or the Issues
// display preferences. The pages share one record through a
// subscribable store so switching layout re-renders every mounted list at
// once without a refetch.

import * as React from "react";

import { DEFAULT_MODULES_LAYOUT, parseModulesLayout, type ModulesLayout } from "./moduleLayouts.js";

/** Browser record holding every project's modules-list layout. */
export const MODULES_LIST_PREFS_STORAGE_KEY = "modules_list_layout";

/** The store shape: project id → chosen layout. */
export type ModulesLayoutRecord = Record<string, ModulesLayout>;

/** Storage accessors a caller can substitute (tests, SSR, a refused browser). */
export interface StorageLike {
  getItem: (key: string) => string | null;
  setItem: (key: string, value: string) => void;
}

function defaultStorage(): StorageLike | undefined {
  try {
    return globalThis.localStorage;
  } catch {
    return undefined;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Parse a stored record, dropping unknown ids and unknown layouts. */
export function parseLayoutRecord(raw: string | null): ModulesLayoutRecord {
  if (raw === null) return {};
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return {};
  }
  if (!isRecord(parsed)) return {};
  const record: ModulesLayoutRecord = {};
  for (const [projectId, value] of Object.entries(parsed)) {
    if (typeof value === "string" && projectId !== "") {
      record[projectId] = parseModulesLayout(value);
    }
  }
  return record;
}

/** The full record currently stored (empty for an absent or corrupt one). */
export function readLayoutRecord(storage: StorageLike | undefined = defaultStorage()): ModulesLayoutRecord {
  if (storage === undefined) return {};
  try {
    return parseLayoutRecord(storage.getItem(MODULES_LIST_PREFS_STORAGE_KEY));
  } catch {
    return {};
  }
}

/** One project's remembered layout; list when it has never been chosen. */
export function readProjectLayout(
  projectId: string,
  storage: StorageLike | undefined = defaultStorage()
): ModulesLayout {
  return readLayoutRecord(storage)[projectId] ?? DEFAULT_MODULES_LAYOUT;
}

/** Write one project's layout, keeping every other project's entry. */
export function writeProjectLayout(
  projectId: string,
  layout: ModulesLayout,
  storage: StorageLike | undefined = defaultStorage()
): void {
  if (storage === undefined || projectId === "") return;
  const record = readLayoutRecord(storage);
  try {
    storage.setItem(MODULES_LIST_PREFS_STORAGE_KEY, JSON.stringify({ ...record, [projectId]: layout }));
  } catch {
    // A browser that refuses writes keeps the in-memory choice for this
    // visit; the next reload falls back to the default.
  }
}

export interface ModuleLayoutHandle {
  layout: ModulesLayout;
  setLayout: (layout: ModulesLayout) => void;
}

/**
 * The project's active layout plus its setter. Reads the record on mount,
 * writes through to storage, and notifies subscribers so another mounted
 * list for the same project follows along.
 */
export function useModuleLayout(projectId: string): ModuleLayoutHandle {
  const [layout, setLayoutState] = React.useState<ModulesLayout>(() => readProjectLayout(projectId));

  React.useEffect(() => {
    setLayoutState(readProjectLayout(projectId));
  }, [projectId]);

  const setLayout = React.useCallback(
    (next: ModulesLayout) => {
      writeProjectLayout(projectId, next);
      setLayoutState(next);
    },
    [projectId]
  );

  return { layout, setLayout };
}
