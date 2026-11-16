// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles no-match view (parity/specs/cycles.md §5, CYC-009). When the
// project has cycles but search and filters hide all of them, the view
// names the cause: search (alone or combined) offers to clear the
// search, filters alone offer the filters clear-all. Clearing one
// dimension never clears the other. Never conflated with the zero
// state.

import { Button } from "@pidash/kit";
import * as React from "react";

export type CyclesNoMatchVariant = "search" | "filters";

export interface CyclesNoMatchProps {
  variant: CyclesNoMatchVariant;
  onClearSearch: () => void;
  /** Clears every filter dimension. Slot owner: NEWFRONT-709. */
  onClearFilters?: (() => void) | undefined;
}

const NO_MATCH_HINT: Record<CyclesNoMatchVariant, string> = {
  search: "Remove the search criteria to see all cycles",
  filters: "Remove the filters to see all cycles",
};

export function CyclesNoMatch({ variant, onClearSearch, onClearFilters }: CyclesNoMatchProps): React.ReactElement {
  const search = variant === "search";
  return (
    <div
      data-testid="cycles-no-match"
      data-variant={variant}
      className="flex flex-col items-center gap-(--space-2) px-(--space-4) py-(--space-12) text-center"
    >
      <h5 className="text-emphasis text-(--text)">No matching cycles</h5>
      <p className="text-body text-(--text-muted)">{NO_MATCH_HINT[variant]}</p>
      <Button variant="secondary" size="small" onClick={search ? onClearSearch : () => onClearFilters?.()}>
        {search ? "Clear search" : "Clear all filters"}
      </Button>
    </div>
  );
}
