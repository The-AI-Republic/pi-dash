// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Projects-list display state (SHELL-028 through SHELL-031). Sort,
// filters and search narrow the fetched collection on the client; each
// workspace keeps its own sort and filters in memory, so a reload
// falls back to creation order. Reset on sign-out like every store.

import { create } from "zustand";

import { registerStoreReset } from "../../../core/session/store.js";
import {
  DEFAULT_LIST_SORT,
  freshListFilters,
  type ListFilters,
  type ListSort,
  type PersonFilterDimension,
  type ValueFilterDimension,
} from "../lib/list-display.js";

export interface WorkspaceListDisplay {
  sort: ListSort;
  filters: ListFilters;
}

function freshDisplay(): WorkspaceListDisplay {
  return { sort: { ...DEFAULT_LIST_SORT }, filters: freshListFilters() };
}

interface ListDisplayState {
  displays: Record<string, WorkspaceListDisplay>;
  /** Live search text; shared until the workspace changes. */
  searchQuery: string;
  activeWorkspace: string | null;
  /** This workspace's sort and filters, defaulted without writing. */
  displayFor: (workspaceSlug: string) => WorkspaceListDisplay;
  setSort: (workspaceSlug: string, sort: ListSort) => void;
  setMyProjects: (workspaceSlug: string, on: boolean) => void;
  toggleAccess: (workspaceSlug: string, value: number) => void;
  togglePerson: (workspaceSlug: string, dimension: PersonFilterDimension, id: string) => void;
  /** Creation dates single-select: the pick replaces the prior one. */
  selectCreated: (workspaceSlug: string, values: string[]) => void;
  removeFilterValue: (workspaceSlug: string, dimension: ValueFilterDimension, value: string) => void;
  removeFilterDimension: (workspaceSlug: string, dimension: ValueFilterDimension) => void;
  /** Clear-all: filters go, sort and search stay. */
  clearListFilters: (workspaceSlug: string) => void;
  setSearchQuery: (query: string) => void;
  /** A workspace switch drops the search text. */
  trackWorkspace: (workspaceSlug: string) => void;
}

function withDisplay(
  displays: Record<string, WorkspaceListDisplay>,
  workspaceSlug: string,
  patch: Partial<WorkspaceListDisplay>
): Record<string, WorkspaceListDisplay> {
  return { ...displays, [workspaceSlug]: { ...(displays[workspaceSlug] ?? freshDisplay()), ...patch } };
}

function toggled(values: string[], value: string): string[] {
  return values.includes(value) ? values.filter((entry) => entry !== value) : [...values, value];
}

const initial = { displays: {}, searchQuery: "", activeWorkspace: null as string | null };

export const useListDisplayStore = create<ListDisplayState>()((set, get) => ({
  ...initial,
  displayFor: (workspaceSlug: string) => get().displays[workspaceSlug] ?? freshDisplay(),
  setSort: (workspaceSlug, sort) =>
    set((state) => ({ displays: withDisplay(state.displays, workspaceSlug, { sort: { ...sort } }) })),
  setMyProjects: (workspaceSlug, on) =>
    set((state) => {
      const display = state.displays[workspaceSlug] ?? freshDisplay();
      return {
        displays: withDisplay(state.displays, workspaceSlug, { filters: { ...display.filters, myProjects: on } }),
      };
    }),
  toggleAccess: (workspaceSlug, value) =>
    set((state) => {
      const display = state.displays[workspaceSlug] ?? freshDisplay();
      const access = display.filters.access.includes(value)
        ? display.filters.access.filter((entry) => entry !== value)
        : [...display.filters.access, value];
      return { displays: withDisplay(state.displays, workspaceSlug, { filters: { ...display.filters, access } }) };
    }),
  togglePerson: (workspaceSlug, dimension, id) =>
    set((state) => {
      const display = state.displays[workspaceSlug] ?? freshDisplay();
      return {
        displays: withDisplay(state.displays, workspaceSlug, {
          filters: { ...display.filters, [dimension]: toggled(display.filters[dimension], id) },
        }),
      };
    }),
  selectCreated: (workspaceSlug, values) =>
    set((state) => {
      const display = state.displays[workspaceSlug] ?? freshDisplay();
      return {
        displays: withDisplay(state.displays, workspaceSlug, {
          filters: { ...display.filters, createdAt: [...values] },
        }),
      };
    }),
  removeFilterValue: (workspaceSlug, dimension, value) =>
    set((state) => {
      const display = state.displays[workspaceSlug] ?? freshDisplay();
      if (dimension === "access") {
        const access = display.filters.access.filter((entry) => `${entry}` !== value);
        return { displays: withDisplay(state.displays, workspaceSlug, { filters: { ...display.filters, access } }) };
      }
      const rest = display.filters[dimension].filter((entry) => entry !== value);
      return {
        displays: withDisplay(state.displays, workspaceSlug, { filters: { ...display.filters, [dimension]: rest } }),
      };
    }),
  removeFilterDimension: (workspaceSlug, dimension) =>
    set((state) => {
      const display = state.displays[workspaceSlug] ?? freshDisplay();
      if (dimension === "access") {
        return {
          displays: withDisplay(state.displays, workspaceSlug, { filters: { ...display.filters, access: [] } }),
        };
      }
      return {
        displays: withDisplay(state.displays, workspaceSlug, { filters: { ...display.filters, [dimension]: [] } }),
      };
    }),
  clearListFilters: (workspaceSlug) =>
    set((state) => ({
      displays: withDisplay(state.displays, workspaceSlug, { filters: freshListFilters() }),
    })),
  setSearchQuery: (query) => set({ searchQuery: query }),
  trackWorkspace: (workspaceSlug) =>
    set((state) =>
      state.activeWorkspace === workspaceSlug ? {} : { activeWorkspace: workspaceSlug, searchQuery: "" }
    ),
}));

/** Clear the list display state (the sign-out reset registry invokes this). */
export function resetListDisplay(): void {
  useListDisplayStore.setState({ displays: {}, searchQuery: "", activeWorkspace: null });
}

/**
 * Join the sign-out reset list. Called once per list-view mount (not at
 * module scope here) so this store stays side-effect-free and the
 * initial bundle drops it (and its display math) when only lazy routes
 * use it (NEWFRONT-523, size gate).
 */
export function registerListDisplayReset(): void {
  registerStoreReset(resetListDisplay);
}
