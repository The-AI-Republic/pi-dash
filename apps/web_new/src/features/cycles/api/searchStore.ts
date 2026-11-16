// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Live-list search queries (parity/specs/cycles.md §3, CYC-005). One
// remembered query per project, held in memory: typing filters the
// already loaded rows, and the query survives navigation away and back
// within the area because the store outlives the list mount. Reloads
// and sign-out forget it, like every other in-memory UI state.

import { create } from "zustand";

import { registerStoreReset } from "../../../core/session/index.js";

interface CyclesSearchUiState {
  queries: Record<string, string>;
  queryFor: (projectId: string) => string;
  setQuery: (projectId: string, query: string) => void;
  clearQuery: (projectId: string) => void;
}

export const useCycleSearchStore = create<CyclesSearchUiState>()((set, get) => ({
  queries: {},
  queryFor: (projectId: string) => get().queries[projectId] ?? "",
  setQuery: (projectId: string, query: string) =>
    set((state) => ({ queries: { ...state.queries, [projectId]: query } })),
  clearQuery: (projectId: string) =>
    set((state) => {
      if (!(projectId in state.queries)) return state;
      const next = { ...state.queries };
      delete next[projectId];
      return { queries: next };
    }),
}));

/** Forget every remembered query; sign-out and tests call this. */
export function resetCycleSearchStore(): void {
  useCycleSearchStore.setState({ queries: {} });
}

registerStoreReset(resetCycleSearchStore);
