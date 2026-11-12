// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sub-issues widget UI state (parity/specs/issues.md §15, ISS-173/178, and
// §23 ISS-234). Every parent keeps its own filters, display settings and
// open flag, and every expanded row keeps its own state: the section's
// open toggle persists per widget, and one parent's filtering never
// touches another parent's list. The edition-only dimensions belong to
// the same record as the base filters, so they survive navigation, feed
// the empty state and the trigger count, and clear with the rest.
// Sign-out resets the whole map through the session registry.

import { create } from "zustand";

import { registerStoreReset } from "../../../core/session/index.js";
import { emptyEditionFilters, toggleEditionFilter, type EditionFilters } from "../edition/filterModel.js";
import {
  defaultSubIssueDisplay,
  defaultDisplayProperties,
  emptySubIssueFilters,
  type SubIssueDisplay,
  type SubIssueDisplayProperties,
  type SubIssueFilterKey,
  type SubIssueFilters,
  type SubIssueGroupBy,
  type SubIssueOrderBy,
  type SubIssuePropertyKey,
} from "./rules.js";

/** Everything one parent remembers. */
export interface ParentSubIssueState {
  /** The collapsible is open by default; the flag persists per widget. */
  open: boolean;
  filters: SubIssueFilters;
  /** Team-project and work-item-type values (ISS-234); empty in the community build. */
  editionFilters: EditionFilters;
  display: SubIssueDisplay;
}

interface SubIssuesUiState {
  parents: Record<string, ParentSubIssueState>;
  stateFor: (parentId: string) => ParentSubIssueState;
  setOpen: (parentId: string, open: boolean) => void;
  toggleOpen: (parentId: string) => void;
  toggleFilter: (parentId: string, key: SubIssueFilterKey, value: string) => void;
  setFilterValues: (parentId: string, key: SubIssueFilterKey, values: readonly string[]) => void;
  toggleEditionFilter: (parentId: string, key: keyof EditionFilters, value: string) => void;
  resetFilters: (parentId: string) => void;
  setGroupBy: (parentId: string, groupBy: SubIssueGroupBy) => void;
  setOrderBy: (parentId: string, orderBy: SubIssueOrderBy) => void;
  toggleProperty: (parentId: string, key: SubIssuePropertyKey) => void;
}

const INITIAL_PARENT: ParentSubIssueState = {
  open: true,
  filters: emptySubIssueFilters(),
  editionFilters: emptyEditionFilters(),
  display: defaultSubIssueDisplay(),
};

/** A blank parent record; every caller gets fresh arrays and maps. */
export function createParentState(): ParentSubIssueState {
  return {
    open: INITIAL_PARENT.open,
    filters: emptySubIssueFilters(),
    editionFilters: emptyEditionFilters(),
    display: { groupBy: null, orderBy: "-created_at", properties: defaultDisplayProperties() },
  };
}

function patchParent(
  parents: Record<string, ParentSubIssueState>,
  parentId: string,
  patch: (current: ParentSubIssueState) => ParentSubIssueState
): Record<string, ParentSubIssueState> {
  const current = parents[parentId] ?? createParentState();
  return { ...parents, [parentId]: patch(current) };
}

export const useSubIssuesUiStore = create<SubIssuesUiState>()((set, get) => ({
  parents: {},
  stateFor: (parentId: string) => get().parents[parentId] ?? INITIAL_PARENT,
  setOpen: (parentId, open) =>
    set((state) => ({ parents: patchParent(state.parents, parentId, (row) => ({ ...row, open })) })),
  toggleOpen: (parentId) =>
    set((state) => ({
      parents: patchParent(state.parents, parentId, (row) => ({ ...row, open: !row.open })),
    })),
  toggleFilter: (parentId, key, value) =>
    set((state) => ({
      parents: patchParent(state.parents, parentId, (row) => {
        const current = row.filters[key];
        const next = current.includes(value) ? current.filter((entry) => entry !== value) : [...current, value];
        return { ...row, filters: { ...row.filters, [key]: next } };
      }),
    })),
  setFilterValues: (parentId, key, values) =>
    set((state) => ({
      parents: patchParent(state.parents, parentId, (row) => ({
        ...row,
        filters: { ...row.filters, [key]: [...values] },
      })),
    })),
  toggleEditionFilter: (parentId, key, value) =>
    set((state) => ({
      parents: patchParent(state.parents, parentId, (row) => ({
        ...row,
        editionFilters: toggleEditionFilter(row.editionFilters, key, value),
      })),
    })),
  resetFilters: (parentId) =>
    set((state) => ({
      parents: patchParent(state.parents, parentId, (row) => ({
        ...row,
        filters: emptySubIssueFilters(),
        editionFilters: emptyEditionFilters(),
      })),
    })),
  setGroupBy: (parentId, groupBy) =>
    set((state) => ({
      parents: patchParent(state.parents, parentId, (row) => ({
        ...row,
        display: { ...row.display, groupBy },
      })),
    })),
  setOrderBy: (parentId, orderBy) =>
    set((state) => ({
      parents: patchParent(state.parents, parentId, (row) => ({
        ...row,
        display: { ...row.display, orderBy },
      })),
    })),
  toggleProperty: (parentId, key) =>
    set((state) => ({
      parents: patchParent(state.parents, parentId, (row) => {
        const properties: SubIssueDisplayProperties = {
          ...row.display.properties,
          [key]: row.display.properties[key] === false,
        };
        return { ...row, display: { ...row.display, properties } };
      }),
    })),
}));

/** Read one parent's record without creating it. */
export function selectParentSubIssueState(parentId: string) {
  return (state: SubIssuesUiState): ParentSubIssueState => state.parents[parentId] ?? INITIAL_PARENT;
}

registerStoreReset(() => {
  useSubIssuesUiStore.setState({ parents: {} });
});
