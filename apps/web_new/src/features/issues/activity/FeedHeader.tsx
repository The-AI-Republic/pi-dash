// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Activity header controls (parity/specs/issues.md §16, ISS-195/196). The
// heading, a sort control that flips the order, and a filter popover with
// the four categories and an accent dot while the selection is narrowed.
// The header row is what the parity driver reads its two trigger buttons
// from, so the sort button is a plain icon button and the filter trigger
// wraps the popover.

import { IconButton, Popover, cn } from "@pidash/kit";
import { ArrowDownWideNarrow, Check, ListFilter } from "lucide-react";
import * as React from "react";

import { FEED_FILTERS, FEED_FILTER_LABELS, isFilterNarrowed, type FeedFilter, type FeedSortOrder } from "./model.js";

export const ACTIVITY_HEADING = "Activity";
export const SORT_LABEL = "Change the activity order";
export const FILTER_LABEL = "Filter activity";

export interface FeedHeaderProps {
  filters: readonly FeedFilter[];
  onToggleFilter: (filter: FeedFilter) => void;
  order: FeedSortOrder;
  onToggleSort: () => void;
  /** Toggles the menu offers; cloud builds pass all five (ISS-206). */
  availableFilters?: readonly FeedFilter[];
  /** Optional leading control, ahead of the sort button (the log-time button in cloud). */
  leading?: React.ReactNode;
}

export function FeedHeader({
  filters,
  onToggleFilter,
  order,
  onToggleSort,
  availableFilters = FEED_FILTERS,
  leading,
}: FeedHeaderProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const narrowed = isFilterNarrowed(filters, availableFilters);

  return (
    <div className="flex items-center justify-between gap-(--space-2)">
      <h2 className="text-h4 text-(--text)">{ACTIVITY_HEADING}</h2>
      <div className="flex items-center gap-(--space-1)">
        {leading}
        <IconButton
          label={order === "asc" ? `${SORT_LABEL}: oldest first` : `${SORT_LABEL}: newest first`}
          aria-pressed={order === "desc"}
          onClick={onToggleSort}
        >
          {/* One glyph for both orders: the flip reads as "oldest first" versus "newest first". */}
          <ArrowDownWideNarrow
            size={16}
            strokeWidth={1.5}
            aria-hidden="true"
            className={order === "asc" ? "rotate-180" : undefined}
          />
        </IconButton>
        <Popover
          open={open}
          onOpenChange={setOpen}
          className="w-48"
          trigger={
            <button
              type="button"
              aria-label={FILTER_LABEL}
              aria-expanded={open}
              className="relative inline-flex size-6 items-center justify-center rounded-(--radius-control) text-(--text-muted) transition-colors hover:bg-(--subtle) hover:text-(--text) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:outline-none"
            >
              <ListFilter size={16} strokeWidth={1.5} aria-hidden="true" />
              {narrowed ? (
                <>
                  <span aria-hidden="true" className="absolute -top-0.5 -right-0.5 size-2 rounded-full bg-(--accent)" />
                  <span className="sr-only">Filters narrowed</span>
                </>
              ) : null}
            </button>
          }
        >
          <div role="group" aria-label="Activity categories" className="flex flex-col">
            {availableFilters.map((filter) => {
              const selected = filters.includes(filter);
              return (
                <button
                  key={filter}
                  type="button"
                  role="checkbox"
                  aria-checked={selected}
                  onClick={() => {
                    onToggleFilter(filter);
                  }}
                  className="text-meta flex items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left hover:bg-(--subtle)"
                >
                  <span
                    className={cn(
                      "flex size-3 items-center justify-center rounded-xs border",
                      selected ? "border-(--accent) bg-(--accent) text-(--on-accent)" : "border-(--border)"
                    )}
                  >
                    {selected ? <Check size={10} strokeWidth={3} aria-hidden="true" /> : null}
                  </span>
                  <span className={selected ? "text-(--text)" : "text-(--text-muted)"}>
                    {FEED_FILTER_LABELS[filter]}
                  </span>
                </button>
              );
            })}
          </div>
        </Popover>
      </div>
    </div>
  );
}
