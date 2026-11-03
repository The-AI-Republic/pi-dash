// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The modules list context (parity/specs/modules.md §2.5, MOD-005). The
// header writes the search, ordering and filters; the body and later
// slices (the timeline's manual reorder in §6.3) read them. The chosen
// sort is exposed here so a nested surface never re-derives or forks the
// ordering rule: timeline reordering is enabled only when `sort.key` is
// `manual`.

import * as React from "react";

import type { ModuleListFilters, ModuleSort } from "../lib/moduleListControls.js";

export interface ModuleListContextValue {
  query: string;
  setQuery: (query: string) => void;
  sort: ModuleSort;
  setSort: (sort: ModuleSort) => void;
  filters: ModuleListFilters;
  setFilters: (filters: ModuleListFilters) => void;
  /** True while any filter dimension carries a value. */
  filtersActive: boolean;
}

const ModuleListContext = React.createContext<ModuleListContextValue | null>(null);

export interface ModuleListProviderProps {
  value: ModuleListContextValue;
  children: React.ReactNode;
}

export function ModuleListProvider({ value, children }: ModuleListProviderProps): React.ReactElement {
  return <ModuleListContext.Provider value={value}>{children}</ModuleListContext.Provider>;
}

/** The list controls, or null outside a modules list (peek, detail). */
export function useModuleListContext(): ModuleListContextValue | null {
  return React.useContext(ModuleListContext);
}
