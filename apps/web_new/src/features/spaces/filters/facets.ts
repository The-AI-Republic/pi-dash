// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public-board filter model (parity/specs/space.md §3.5, SPC-034–039).
// Pure functions only: which facets a layout offers, in which order the
// applied strip renders them, and how each removal scope rewrites the
// filter object. Address spelling (singular comma-joined keys) is settled
// by lib/address; this module never touches the router.

import { SPACE_PRIORITIES, type SpaceBoardFilters, type SpaceBoardSearch, type SpaceLayout } from "../lib/address.js";
import type { SpaceMessageKey } from "../lib/spaceMessages.js";

/** Facets the filter panel can offer (labels arrive via the address only). */
export type SpacePanelFacet = "priority" | "state";

/** Every facet the applied strip can render, panel-offered or not. */
export type SpaceStripFacet = SpacePanelFacet | "labels";

export type SpacePriorityLevel = (typeof SPACE_PRIORITIES)[number];

/**
 * The facets each public layout supports, consulted by the panel instead
 * of hard-wired sections (SPC-038). Both layouts support the same pair
 * today; the table keeps holding if a layout is added.
 */
export const SPACE_LAYOUT_FILTER_FACETS: Record<SpaceLayout, readonly SpacePanelFacet[]> = {
  list: ["priority", "state"],
  kanban: ["priority", "state"],
};

/** The order facet shells render on the applied strip (SPC-035). */
export const SPACE_STRIP_FACET_ORDER: readonly SpaceStripFacet[] = ["priority", "state", "labels"];

/** State options shown before the expand control (SPC-034). */
export const SPACE_STATE_OPTION_PREVIEW = 5;

/** Translated shell heading per facet (SPC-065). */
export const SPACE_FACET_MESSAGE_KEYS: Record<SpaceStripFacet, SpaceMessageKey> = {
  priority: "filters.priority",
  state: "filters.state",
  labels: "filters.labels",
};

/** Translated option name per priority level (SPC-065). */
export const SPACE_PRIORITY_MESSAGE_KEYS: Record<SpacePriorityLevel, SpaceMessageKey> = {
  urgent: "filters.priority.urgent",
  high: "filters.priority.high",
  medium: "filters.priority.medium",
  low: "filters.priority.low",
  none: "filters.priority.none",
};

export function panelFacetsForLayout(layout: SpaceLayout): readonly SpacePanelFacet[] {
  return SPACE_LAYOUT_FILTER_FACETS[layout];
}

/** The values applied under one facet (empty when the facet is clear). */
export function facetValues(filters: SpaceBoardFilters, facet: SpaceStripFacet): readonly string[] {
  return filters[facet] ?? [];
}

/** Whether any facet carries values (the strip renders only then). */
export function hasAppliedFilters(filters: SpaceBoardFilters): boolean {
  return SPACE_STRIP_FACET_ORDER.some((facet) => (filters[facet]?.length ?? 0) > 0);
}

function withFacetValues(
  filters: SpaceBoardFilters,
  facet: SpaceStripFacet,
  values: readonly string[] | undefined
): SpaceBoardFilters {
  const next = { ...filters };
  if (values === undefined || values.length === 0) {
    delete next[facet];
  } else {
    next[facet] = values;
  }
  return next;
}

/** Panel pick: add the value, or drop it when it is already applied. */
export function toggleFilterValue(
  filters: SpaceBoardFilters,
  facet: SpacePanelFacet,
  value: string
): SpaceBoardFilters {
  const current = filters[facet] ?? [];
  const next = current.includes(value) ? current.filter((entry) => entry !== value) : [...current, value];
  return withFacetValues(filters, facet, next);
}

/**
 * Pill remove: drop one value, leaving the facet (and its address key)
 * otherwise alone — survivors stay under the singular key (SPC-035).
 */
export function removeFilterValue(
  filters: SpaceBoardFilters,
  facet: SpaceStripFacet,
  value: string
): SpaceBoardFilters {
  const current = filters[facet] ?? [];
  if (!current.includes(value)) return filters;
  return withFacetValues(
    filters,
    facet,
    current.filter((entry) => entry !== value)
  );
}

/** Facet clear: drop a whole facet, keeping the others. */
export function clearFilterFacet(filters: SpaceBoardFilters, facet: SpaceStripFacet): SpaceBoardFilters {
  return withFacetValues(filters, facet, undefined);
}

/** Clear-all: keep the view and the open issue, drop every facet (SPC-036). */
export function clearedFilterSearch(search: SpaceBoardSearch): SpaceBoardSearch {
  return {
    ...(search.board === undefined ? {} : { board: search.board }),
    ...(search.peekId === undefined ? {} : { peekId: search.peekId }),
  };
}

/** The board search for a filter object: view + open issue + facets. */
export function filteredBoardSearch(search: SpaceBoardSearch, filters: SpaceBoardFilters): SpaceBoardSearch {
  return {
    ...(search.board === undefined ? {} : { board: search.board }),
    ...(search.peekId === undefined ? {} : { peekId: search.peekId }),
    ...(filters.state === undefined ? {} : { state: filters.state }),
    ...(filters.priority === undefined ? {} : { priority: filters.priority }),
    ...(filters.labels === undefined ? {} : { labels: filters.labels }),
  };
}

/** Case-insensitive substring match over one option's readable texts (SPC-039). */
export function optionMatchesQuery(candidates: readonly string[], query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (needle === "") return true;
  return candidates.some((text) => text.toLowerCase().includes(needle));
}

/** The state options shown: everything once expanded, the preview otherwise. */
export function previewStateOptions<T>(options: readonly T[], expanded: boolean): readonly T[] {
  return expanded ? options : options.slice(0, SPACE_STATE_OPTION_PREVIEW);
}
