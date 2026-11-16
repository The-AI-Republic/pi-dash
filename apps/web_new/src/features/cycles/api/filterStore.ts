// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Live-list filter values (parity/specs/cycles.md §4, CYC-006–008). One
// filter set per project, held in memory beside the remembered search
// query: the menu, the chips row and the no-match clear-all all read
// this one model, and clearing it never touches the search text. The
// list address carries no filter keys, so reloads forget the values
// like every other in-memory UI state.

import * as React from "react";
import type { CycleRow as CycleRowData } from "@pidash/api-client";
import { create } from "zustand";

import { registerStoreReset } from "../../../core/session/index.js";
import { useProjectRow } from "../../auth/index.js";
import {
  EMPTY_CYCLE_FILTERS,
  hasCycleFilters,
  makeCycleFilter,
  type CycleDateFilterDimension,
  type CycleFilterDimension,
  type CycleListFilters,
} from "../lib/cycleFilters.js";

interface CyclesFilterUiState {
  filters: Record<string, CycleListFilters>;
  filtersFor: (projectId: string) => CycleListFilters;
  toggleStatus: (projectId: string, value: string) => void;
  setDateValues: (projectId: string, dimension: CycleDateFilterDimension, values: readonly string[]) => void;
  removeValue: (projectId: string, dimension: CycleFilterDimension, value: string) => void;
  clearDimension: (projectId: string, dimension: CycleFilterDimension) => void;
  clearAll: (projectId: string) => void;
}

function projectFilters(state: CyclesFilterUiState, projectId: string): CycleListFilters {
  return state.filters[projectId] ?? EMPTY_CYCLE_FILTERS;
}

export const useCycleFilterStore = create<CyclesFilterUiState>()((set, get) => ({
  filters: {},
  filtersFor: (projectId: string) => projectFilters(get(), projectId),
  toggleStatus: (projectId: string, value: string) =>
    set((state) => {
      const current = projectFilters(state, projectId);
      const status = current.status.includes(value)
        ? current.status.filter((entry) => entry !== value)
        : [...current.status, value];
      return { filters: { ...state.filters, [projectId]: { ...current, status } } };
    }),
  setDateValues: (projectId: string, dimension: CycleDateFilterDimension, values: readonly string[]) =>
    set((state) => {
      const current = projectFilters(state, projectId);
      return { filters: { ...state.filters, [projectId]: { ...current, [dimension]: [...values] } } };
    }),
  removeValue: (projectId: string, dimension: CycleFilterDimension, value: string) =>
    set((state) => {
      const current = projectFilters(state, projectId);
      return {
        filters: {
          ...state.filters,
          [projectId]: { ...current, [dimension]: current[dimension].filter((entry) => entry !== value) },
        },
      };
    }),
  clearDimension: (projectId: string, dimension: CycleFilterDimension) =>
    set((state) => {
      const current = projectFilters(state, projectId);
      if (current[dimension].length === 0) return state;
      return { filters: { ...state.filters, [projectId]: { ...current, [dimension]: [] } } };
    }),
  clearAll: (projectId: string) =>
    set((state) => {
      if (!(projectId in state.filters)) return state;
      const next = { ...state.filters };
      delete next[projectId];
      return { filters: next };
    }),
}));

/** Forget every project's filters; sign-out and tests call this. */
export function resetCycleFilterStore(): void {
  useCycleFilterStore.setState({ filters: {} });
}

registerStoreReset(resetCycleFilterStore);

/** The live list's filter wiring: predicate, badge state, and clear-all. */
export interface CycleListFilter {
  /** Row predicate for the current values and the project timezone. */
  predicate: (cycle: CycleRowData) => boolean;
  /** Whether any dimension narrows the list. */
  active: boolean;
  /** Clear every dimension; the search query survives. */
  clearAll: () => void;
}

/**
 * Read the project's filter set with the timezone its date windows
 * resolve in. The predicate is memoized on both, so the list only
 * re-narrows when values change or the project read lands.
 */
export function useCycleListFilter(workspaceSlug: string, projectId: string): CycleListFilter {
  const filters = useCycleFilterStore((state) => state.filtersFor(projectId));
  const project = useProjectRow(workspaceSlug, projectId);
  const timeZone = project.data?.timezone ?? "UTC";
  const predicate = React.useMemo(() => makeCycleFilter(filters, timeZone), [filters, timeZone]);
  const clearAll = React.useCallback(() => {
    useCycleFilterStore.getState().clearAll(projectId);
  }, [projectId]);
  return { predicate, active: hasCycleFilters(filters), clearAll };
}
