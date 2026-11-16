// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// No rows match the active search or filters (parity/specs/pages.md
// §3.7, PAGE-003 and PAGE-005). Clear the dimension that can restore
// rows; no create action appears here.

import { EmptyState } from "@pidash/kit";
import * as React from "react";

export interface PagesNoResultsProps {
  search: boolean;
  filters: boolean;
  onClearSearch: () => void;
  onClearFilters: () => void;
}

export function PagesNoResults({
  search,
  filters,
  onClearSearch,
  onClearFilters,
}: PagesNoResultsProps): React.ReactElement {
  const action =
    search && !filters
      ? { label: "Clear search", onSelect: onClearSearch }
      : filters && !search
        ? { label: "Clear filters", onSelect: onClearFilters }
        : {
            label: "Clear all filters",
            onSelect: () => {
              onClearSearch();
              onClearFilters();
            },
          };
  return (
    <div data-testid="pages-no-results">
      <EmptyState
        variant="detailed"
        art={{ category: "general", type: "no-results" }}
        title="No matching results."
        description="Try a different title or remove a filter."
        primaryAction={action}
      />
    </div>
  );
}
