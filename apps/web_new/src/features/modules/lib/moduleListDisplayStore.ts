// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-project modules-list display state (parity/specs/modules.md
// §2.4–§2.6 and §13.3.1, MOD-004/005/006). The list is project-scoped, so
// its name search, ordering and filter selection live in the modules
// feature's own browser record — one entry per project — beside the layout
// record. They are personal, restored on reload, never written to the
// project row, a module's user-properties record or the Issues display
// preferences, and never sent to the server. Zod validates every stored
// entry, so a corrupt value falls back to the documented defaults.

import * as React from "react";
import * as z from "zod/mini";

import type { StorageLike } from "./listLayoutStore.js";
import {
  DEFAULT_MODULE_SORT,
  emptyModuleFilters,
  MODULE_SORT_DIRECTIONS,
  MODULE_SORT_KEYS,
  MODULE_DATE_PRESETS,
  MODULE_STATUS_OPTIONS,
  type ModuleDateToken,
  type ModuleListFilters,
  type ModuleSort,
} from "./moduleListControls.js";

/** Browser record holding every project's list search/sort/filters. */
export const MODULES_LIST_DISPLAY_STORAGE_KEY = "modules_list_display";

const displaySchema = z.object({
  query: z.string(),
  sort: z.object({ key: z.enum(MODULE_SORT_KEYS), direction: z.enum(MODULE_SORT_DIRECTIONS) }),
  filters: z.object({
    favorites: z.boolean(),
    status: z.array(z.enum(MODULE_STATUS_OPTIONS.map((option) => option.value))),
    lead: z.array(z.string()),
    members: z.array(z.string()),
    "start-date": z.array(
      z.union([
        z.object({ kind: z.literal("preset"), value: z.enum(MODULE_DATE_PRESETS.map((option) => option.value)) }),
        z.object({ kind: z.literal("range"), value: z.object({ start: z.iso.date(), end: z.iso.date() }) }),
      ])
    ),
    "due-date": z.array(
      z.union([
        z.object({ kind: z.literal("preset"), value: z.enum(MODULE_DATE_PRESETS.map((option) => option.value)) }),
        z.object({ kind: z.literal("range"), value: z.object({ start: z.iso.date(), end: z.iso.date() }) }),
      ])
    ),
  }),
});

/** One project's remembered list display. */
export interface ModuleListDisplay {
  query: string;
  sort: ModuleSort;
  filters: ModuleListFilters;
}

/** The record shape: project id → display state. */
export type ModuleListDisplayRecord = Record<string, ModuleListDisplay>;

function defaults(): ModuleListDisplay {
  return { query: "", sort: { ...DEFAULT_MODULE_SORT }, filters: emptyModuleFilters() };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function defaultStorage(): StorageLike | undefined {
  try {
    return globalThis.localStorage;
  } catch {
    return undefined;
  }
}

/** Copy a validated entry into the runtime shape with fresh arrays. */
function toDisplay(entry: z.infer<typeof displaySchema>): ModuleListDisplay {
  return {
    query: entry.query,
    sort: { key: entry.sort.key, direction: entry.sort.direction },
    filters: {
      favorites: entry.filters.favorites,
      status: [...entry.filters.status],
      lead: [...entry.filters.lead],
      members: [...entry.filters.members],
      "start-date": entry.filters["start-date"] as ModuleDateToken[],
      "due-date": entry.filters["due-date"] as ModuleDateToken[],
    },
  };
}

/** Parse a stored record, dropping unknown ids and invalid entries. */
export function parseDisplayRecord(raw: string | null): ModuleListDisplayRecord {
  if (raw === null) return {};
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return {};
  }
  if (!isRecord(parsed)) return {};
  const record: ModuleListDisplayRecord = {};
  for (const [projectId, value] of Object.entries(parsed)) {
    const result = displaySchema.safeParse(value);
    if (projectId !== "" && result.success) record[projectId] = toDisplay(result.data);
  }
  return record;
}

/** The full record currently stored (empty for an absent or corrupt one). */
export function readDisplayRecord(storage: StorageLike | undefined = defaultStorage()): ModuleListDisplayRecord {
  if (storage === undefined) return {};
  try {
    return parseDisplayRecord(storage.getItem(MODULES_LIST_DISPLAY_STORAGE_KEY));
  } catch {
    return {};
  }
}

/** One project's remembered display; the documented defaults when never chosen. */
export function readProjectDisplay(
  projectId: string,
  storage: StorageLike | undefined = defaultStorage()
): ModuleListDisplay {
  return readDisplayRecord(storage)[projectId] ?? defaults();
}

// ---------------------------------------------------------------- live store

type Listener = () => void;
const listeners = new Set<Listener>();
/** The last written value per project; a refused write keeps this visit's choice. */
const memory = new Map<string, ModuleListDisplay>();
/** Values read back from storage, so repeated renders share one reference. */
const readCache = new Map<string, ModuleListDisplay>();

function notify(): void {
  readCache.clear();
  for (const listener of listeners) listener();
}

function subscribe(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Write one project's display, keeping every other project's entry. */
export function writeProjectDisplay(
  projectId: string,
  display: ModuleListDisplay,
  storage: StorageLike | undefined = defaultStorage()
): void {
  if (storage === undefined || projectId === "") return;
  const record = readDisplayRecord(storage);
  try {
    storage.setItem(MODULES_LIST_DISPLAY_STORAGE_KEY, JSON.stringify({ ...record, [projectId]: display }));
  } catch {
    // A refused write keeps the in-memory choice for this visit.
  }
  notify();
}

/** The project's display, stable between renders until the next write. */
export function currentProjectDisplay(
  projectId: string,
  storage: StorageLike | undefined = defaultStorage()
): ModuleListDisplay {
  const overlay = memory.get(projectId);
  if (overlay !== undefined) return overlay;
  const cached = readCache.get(projectId);
  if (cached !== undefined) return cached;
  const value = readProjectDisplay(projectId, storage);
  readCache.set(projectId, value);
  return value;
}

/** Reset the module-level caches and subscribers (sign-out and tests). */
export function resetModuleDisplayCache(): void {
  memory.clear();
  readCache.clear();
  listeners.clear();
}

export interface ModuleListDisplayHandle {
  display: ModuleListDisplay;
  setQuery: (query: string) => void;
  setSort: (sort: ModuleSort) => void;
  setFilters: (filters: ModuleListFilters) => void;
}

function replace(projectId: string, next: ModuleListDisplay, storage: StorageLike | undefined): void {
  memory.set(projectId, next);
  writeProjectDisplay(projectId, next, storage);
}

/**
 * The project's active search, ordering and filters with setters. Writes
 * through to storage and notifies subscribers so another mounted list for
 * the same project follows along.
 */
export function useModuleListDisplay(
  projectId: string,
  storage: StorageLike | undefined = defaultStorage()
): ModuleListDisplayHandle {
  const display = React.useSyncExternalStore(
    subscribe,
    () => currentProjectDisplay(projectId, storage),
    () => currentProjectDisplay(projectId, storage)
  );
  const update = React.useCallback(
    (patch: Partial<ModuleListDisplay>) =>
      replace(projectId, { ...currentProjectDisplay(projectId, storage), ...patch }, storage),
    [projectId, storage]
  );
  return {
    display,
    setQuery: React.useCallback((query: string) => update({ query }), [update]),
    setSort: React.useCallback((sort: ModuleSort) => update({ sort }), [update]),
    setFilters: React.useCallback((filters: ModuleListFilters) => update({ filters }), [update]),
  };
}
