// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Applied pages filters (parity/specs/pages.md §3.4, PAGE-005). Each
// active dimension renders a removable chip; Clear all removes every
// dimension together. Removing one chip restores only its rows.

import { X } from "lucide-react";
import * as React from "react";

import { pageFilterChips, removePageFilter, type PageChipId, type PageFilters } from "../lib/pageList.js";

export interface PagesFilterChipsProps {
  filters: PageFilters;
  creatorName: string | null;
  onFiltersChange: (filters: PageFilters) => void;
}

export function PagesFilterChips({
  filters,
  creatorName,
  onFiltersChange,
}: PagesFilterChipsProps): React.ReactElement | null {
  const chips = pageFilterChips(filters, creatorName);
  if (chips.length === 0) return null;
  const remove = (id: PageChipId): void => onFiltersChange(removePageFilter(filters, id));
  return (
    <div data-testid="pages-filter-chips" className="flex flex-wrap items-center gap-(--space-2)">
      {chips.map((chip) => (
        <span
          key={chip.id}
          data-testid={`pages-filter-chip-${chip.id}`}
          className="inline-flex items-center gap-(--space-1) rounded-(--radius-control) border border-(--border) px-(--space-2) py-(--space-1)"
        >
          <span className="text-meta text-(--text-muted)">{chip.label}</span>
          <span className="text-meta inline-flex items-center gap-(--space-1) rounded-(--radius-full) bg-(--subtle) px-(--space-2) py-px">
            {chip.value}
          </span>
          <button
            type="button"
            aria-label={`Remove ${chip.label}`}
            onClick={() => remove(chip.id)}
            className="grid cursor-pointer place-items-center rounded-full text-(--text-muted) hover:text-(--text)"
          >
            <X size={12} strokeWidth={2} aria-hidden="true" />
          </button>
        </span>
      ))}
      <button
        type="button"
        onClick={() => onFiltersChange({ favorites: false, created: null, creator: null })}
        className="text-meta cursor-pointer rounded-(--radius-full) border border-(--border) px-(--space-3) py-px hover:bg-(--subtle)"
      >
        Clear all
      </button>
    </div>
  );
}
