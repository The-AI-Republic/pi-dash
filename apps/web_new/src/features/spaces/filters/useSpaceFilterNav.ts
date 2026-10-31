// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Filter address writes (SPC-035/036): every panel pick and every removal
// scope pushes one address under the singular filter keys; clearing drops
// the facet keys and keeps the view and the open issue. The issues query
// is keyed by the filters, so the pushed address refetches page one by
// itself. Pushes (not replaces) so Back returns to the previous filters.

import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { normalizeBoardHref, type SpaceBoardFilters, type SpaceBoardSearch } from "../lib/address.js";
import {
  clearedFilterSearch,
  clearFilterFacet,
  filteredBoardSearch,
  removeFilterValue,
  toggleFilterValue,
  type SpacePanelFacet,
  type SpaceStripFacet,
} from "./facets.js";

export interface SpaceFilterNav {
  /** Panel pick: toggle one value under a panel facet. */
  toggleValue: (facet: SpacePanelFacet, value: string) => void;
  /** Pill remove: drop one value, keeping the survivors. */
  removeValue: (facet: SpaceStripFacet, value: string) => void;
  /** Facet clear: drop a whole facet, keeping the others. */
  clearFacet: (facet: SpaceStripFacet) => void;
  /** Clear-all: drop every facet, keeping the view and the open issue. */
  clearAll: () => void;
}

export function useSpaceFilterNav(
  anchor: string,
  search: SpaceBoardSearch,
  filters: SpaceBoardFilters
): SpaceFilterNav {
  const navigate = useNavigate();
  const navigateRef = React.useRef(navigate);
  navigateRef.current = navigate;

  const pushFilters = React.useCallback(
    (next: SpaceBoardFilters) => {
      void navigateRef.current({ href: normalizeBoardHref(anchor, filteredBoardSearch(search, next)) });
    },
    [anchor, search]
  );

  const toggleValue = React.useCallback(
    (facet: SpacePanelFacet, value: string) => pushFilters(toggleFilterValue(filters, facet, value)),
    [filters, pushFilters]
  );

  const removeValue = React.useCallback(
    (facet: SpaceStripFacet, value: string) => pushFilters(removeFilterValue(filters, facet, value)),
    [filters, pushFilters]
  );

  const clearFacet = React.useCallback(
    (facet: SpaceStripFacet) => pushFilters(clearFilterFacet(filters, facet)),
    [filters, pushFilters]
  );

  const clearAll = React.useCallback(() => {
    void navigateRef.current({ href: normalizeBoardHref(anchor, clearedFilterSearch(search)) });
  }, [anchor, search]);

  return { toggleValue, removeValue, clearFacet, clearAll };
}
