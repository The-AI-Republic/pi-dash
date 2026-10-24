// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages structured filters (parity/specs/pages.md §3.4, PAGE-005).
// Favorites is a direct toggle; creation conditions apply immediately;
// the creator picker searches the workspace directory and narrows to the
// picked owner. All predicates run against the one loaded collection.

import { Avatar, Button, Popover } from "@pidash/kit";
import { ListFilter, Search } from "lucide-react";
import * as React from "react";

import type { PageCreatorOption, PageDateCondition, PageDatePreset, PageFilters } from "../lib/pageList.js";
import { PAGE_DATE_PRESETS } from "../lib/pageList.js";

export interface PagesFilterMenuProps {
  filters: PageFilters;
  creators: readonly PageCreatorOption[];
  currentUserId: string | null;
  onFiltersChange: (filters: PageFilters) => void;
}

const SECTION_TITLE = "text-caption mb-(--space-2) font-medium text-(--text-muted)";

export function PagesFilterMenu({
  filters,
  creators,
  currentUserId,
  onFiltersChange,
}: PagesFilterMenuProps): React.ReactElement {
  const [creatorQuery, setCreatorQuery] = React.useState("");
  const [customDate, setCustomDate] = React.useState("");
  const customDateValue =
    typeof filters.created === "object" && filters.created !== null ? filters.created.custom : null;
  React.useEffect(() => {
    if (customDateValue !== null) setCustomDate(customDateValue);
  }, [customDateValue]);
  const visibleCreators = React.useMemo(
    () =>
      creators
        .map((creator) => (creator.id === currentUserId ? { ...creator, name: "You" } : creator))
        .filter((creator) => creator.name.toLowerCase().includes(creatorQuery.trim().toLowerCase()))
        .sort((left, right) => left.name.localeCompare(right.name)),
    [creators, creatorQuery, currentUserId]
  );

  const chooseDate = (preset: PageDatePreset): void => {
    onFiltersChange({ ...filters, created: filters.created === preset ? null : preset });
  };

  const customActive = customDateValue !== null;
  const chooseCustom = (condition: PageDateCondition): void => {
    onFiltersChange({ ...filters, created: customActive ? null : condition });
  };

  return (
    <Popover
      trigger={
        <Button size="small" variant="ghost" className="text-(--text-muted)">
          <ListFilter size={14} strokeWidth={1.5} aria-hidden="true" />
          <span>Filters</span>
        </Button>
      }
      className="w-80"
    >
      <div data-testid="pages-filter-panel" className="flex flex-col gap-(--space-4)">
        <label className="flex items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) px-(--space-2)">
          <Search size={12} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
          <input
            type="search"
            aria-label="Search filter options"
            placeholder="Search"
            value={creatorQuery}
            onChange={(event) => setCreatorQuery(event.target.value)}
            className="text-body w-full bg-transparent py-(--space-1) text-(--text) outline-none placeholder:text-(--text-muted)"
          />
        </label>
        <section aria-label="Favorites">
          <h3 className={SECTION_TITLE}>Favorites</h3>
          <button
            type="button"
            aria-pressed={filters.favorites}
            onClick={() => onFiltersChange({ ...filters, favorites: !filters.favorites })}
            className="text-body w-full cursor-pointer rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left text-(--text) hover:bg-(--subtle)"
          >
            Favorites
          </button>
        </section>
        <section aria-label="Created At">
          <h3 className={SECTION_TITLE}>Created At</h3>
          <div className="grid grid-cols-2 gap-(--space-1)">
            {PAGE_DATE_PRESETS.map((preset) => (
              <button
                key={preset.value}
                type="button"
                aria-pressed={filters.created === preset.value}
                onClick={() => chooseDate(preset.value)}
                className="text-meta cursor-pointer rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left text-(--text) hover:bg-(--subtle)"
              >
                {preset.label}
              </button>
            ))}
          </div>
          <div className="mt-(--space-2) flex items-center gap-(--space-2)">
            <input
              type="date"
              aria-label="Custom creation date"
              value={customActive ? customDateValue : customDate}
              onChange={(event) => setCustomDate(event.target.value)}
              className="text-meta h-(--control-height) flex-1 rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) text-(--text) focus:border-(--accent) focus:outline-none"
            />
            <button
              type="button"
              aria-pressed={customActive}
              disabled={!customActive && customDate === ""}
              onClick={() => {
                if (customDateValue !== null) {
                  chooseCustom({ custom: customDateValue });
                } else if (customDate !== "") chooseCustom({ custom: customDate });
              }}
              className="text-meta cursor-pointer rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text) hover:bg-(--subtle) disabled:cursor-not-allowed disabled:opacity-50"
            >
              Apply
            </button>
          </div>
        </section>
        <section aria-label="Created by">
          <h3 className={SECTION_TITLE}>Created by</h3>
          <div className="flex max-h-52 flex-col overflow-y-auto">
            {visibleCreators.map((creator) => (
              <button
                key={creator.id}
                type="button"
                aria-pressed={filters.creator === creator.id}
                onClick={() =>
                  onFiltersChange({ ...filters, creator: filters.creator === creator.id ? null : creator.id })
                }
                className="flex cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left hover:bg-(--subtle)"
              >
                <Avatar
                  name={creator.name}
                  size="small"
                  {...(creator.avatarUrl === null ? {} : { src: creator.avatarUrl })}
                />
                <div className="text-body min-w-0 truncate text-(--text)">{creator.name}</div>
              </button>
            ))}
            {visibleCreators.length === 0 ? (
              <p className="text-meta px-(--space-2) py-(--space-1) text-(--text-muted)">No members found.</p>
            ) : null}
          </div>
        </section>
      </div>
    </Popover>
  );
}
