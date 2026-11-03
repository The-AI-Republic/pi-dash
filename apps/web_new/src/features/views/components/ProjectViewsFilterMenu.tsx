// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Filters menu for the project views list (parity/specs/views.md §4,
// VIEW-006). One popover whose search narrows the options (never the
// rows), holding the Favorites switch and the Created date and Created by
// sections. Access is a cloud-only dimension: an edition opts in with the
// view-access-filter flag, and the OSS build renders neither the section
// nor its values. The date section follows the shared filter-section
// rhythm (cap, "View all", "No matches found"); its custom window is the
// shared date-range picker and means exactly the window picked.

import { Button, Popover } from "@pidash/kit";
import type { WorkspaceMember } from "@pidash/api-client";
import { ListFilter, Search, X } from "lucide-react";
import * as React from "react";

import { edition } from "../../../core/edition/index.js";
import {
  DateRangePicker,
  FilterSection,
  useWeekStart,
  type FilterSectionOption,
  type RangeValue,
} from "../../issues/index.js";
import {
  isViewAccessFilterEnabled,
  toggleViewAccessFilter,
  selectViewCustomCreated,
  toggleViewCreatedPreset,
  toggleViewCreatorFilter,
  toggleViewFavoriteFilter,
  VIEW_ACCESS_OPTIONS,
  VIEW_CREATED_CUSTOM_LABEL,
  VIEW_CREATED_CUSTOM_VALUE,
  VIEW_CREATED_PRESETS,
  type ViewListFilters,
} from "../lib/viewListControls.js";

export { isViewAccessFilterEnabled, VIEW_ACCESS_FILTER_FLAG } from "../lib/viewListControls.js";

/** Rows a section shows before offering "View all". */
const OPTION_CAP = 5;

export interface ProjectViewsFilterMenuProps {
  filters: ViewListFilters;
  /** Creator options; undefined while the roster resolves. */
  members: readonly WorkspaceMember[] | undefined;
  /** The signed-in member, labelled "You" among the creators. */
  selfId: string | null;
  onFiltersChange: (filters: ViewListFilters) => void;
}

export function ProjectViewsFilterMenu({
  filters,
  members,
  selfId,
  onFiltersChange,
}: ProjectViewsFilterMenuProps): React.ReactElement {
  const weekStart = useWeekStart();
  // Favorites and a completed custom window are single-value picks, so
  // they close the panel; the multi-select sections leave it open.
  const [open, setOpen] = React.useState(false);
  const [optionSearch, setOptionSearch] = React.useState("");
  const [customOpen, setCustomOpen] = React.useState(false);
  // The window follows the picker while a range is half-picked, so a first
  // click never narrows the list or closes the dropdown.
  const [pendingCustom, setPendingCustom] = React.useState<RangeValue | null>(null);
  const accessOffered = isViewAccessFilterEnabled(edition);

  const activeCount =
    (filters.favorites ? 1 : 0) +
    (accessOffered ? filters.access.length : 0) +
    filters.created.presets.length +
    (filters.created.custom === null ? 0 : 1) +
    filters.creators.length;

  const search = optionSearch.trim().toLowerCase();
  const narrowed = (label: string): boolean => label.toLowerCase().includes(search);

  const favoritesMatch = narrowed("Favorites");
  const accessOptions: FilterSectionOption[] = VIEW_ACCESS_OPTIONS.filter((option) => narrowed(option.label)).map(
    (option) => ({ id: `${option.value}`, label: option.label })
  );
  const dateOptions: FilterSectionOption[] = [
    ...VIEW_CREATED_PRESETS.filter((option) => narrowed(option.label)).map((option) => ({
      id: option.value,
      label: option.label,
    })),
    ...(narrowed(VIEW_CREATED_CUSTOM_LABEL)
      ? [{ id: VIEW_CREATED_CUSTOM_VALUE, label: VIEW_CREATED_CUSTOM_LABEL }]
      : []),
  ];
  const creatorOptions: FilterSectionOption[] =
    members
      ?.filter((entry) => narrowed(entry.member.display_name))
      .map((entry) => ({
        id: entry.member.id,
        label: selfId !== null && entry.member.id === selfId ? "You" : entry.member.display_name,
      })) ?? [];
  const createdApplied = [
    ...filters.created.presets,
    ...(filters.created.custom === null ? [] : [VIEW_CREATED_CUSTOM_VALUE]),
  ];

  const pickCreated = (id: string): void => {
    if (id !== VIEW_CREATED_CUSTOM_VALUE) {
      onFiltersChange(toggleViewCreatedPreset(filters, id as (typeof VIEW_CREATED_PRESETS)[number]["value"]));
      return;
    }
    // The custom row toggles: a second pick clears the window, the first
    // reveals the picker beneath the section.
    if (filters.created.custom !== null) {
      onFiltersChange(selectViewCustomCreated(filters, null));
      setPendingCustom(null);
      setCustomOpen(false);
      return;
    }
    setPendingCustom(filters.created.custom);
    setCustomOpen(true);
  };

  const applyCustom = (range: RangeValue): void => {
    setPendingCustom(range);
    if (range.start === null && range.end === null) {
      onFiltersChange(selectViewCustomCreated(filters, null));
      setCustomOpen(false);
      return;
    }
    // A complete window is the filter; a half-picked one only steers the
    // calendar until the other end arrives.
    if (range.start !== null && range.end !== null) {
      onFiltersChange(selectViewCustomCreated(filters, range));
      setCustomOpen(false);
      setOpen(false);
    }
  };

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) return;
        // The option search and an unpicked window belong to one visit of
        // the panel, not to the filter it applies.
        setOptionSearch("");
        setCustomOpen(false);
        setPendingCustom(null);
      }}
      trigger={
        <Button variant="secondary" size="small" data-testid="project-views-filter-trigger">
          <ListFilter size={14} strokeWidth={2} aria-hidden="true" />
          <span>Filters</span>
          {activeCount > 0 ? (
            <span
              data-testid="project-views-filter-badge"
              aria-hidden="true"
              className="size-1.5 rounded-full bg-(--accent)"
            />
          ) : null}
        </Button>
      }
      className="max-h-[24rem] overflow-y-auto"
    >
      <div data-testid="project-views-filter-panel" className="flex flex-col gap-(--space-2)">
        <div className="flex items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) py-(--space-2)">
          <Search size={14} strokeWidth={2} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
          <input
            type="text"
            aria-label="Search filters"
            placeholder="Search"
            value={optionSearch}
            onChange={(event) => setOptionSearch(event.target.value)}
            className="text-body w-full bg-transparent text-(--text) outline-none placeholder:text-(--text-muted)"
          />
          {optionSearch === "" ? null : (
            <button
              type="button"
              aria-label="Clear filter search"
              onClick={() => setOptionSearch("")}
              className="grid cursor-pointer place-items-center text-(--text-muted)"
            >
              <X size={14} strokeWidth={2} aria-hidden="true" />
            </button>
          )}
        </div>

        {favoritesMatch ? (
          <label className="text-body flex cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) hover:bg-(--subtle)">
            <input
              type="checkbox"
              checked={filters.favorites}
              onChange={() => {
                onFiltersChange(toggleViewFavoriteFilter(filters));
                setOpen(false);
              }}
            />
            Favorites
          </label>
        ) : null}

        {accessOffered ? (
          <FilterSection
            title="Access"
            kind="multi"
            cap={OPTION_CAP}
            applied={filters.access.map((value) => `${value}`)}
            options={accessOptions}
            onToggle={(id) => onFiltersChange(toggleViewAccessFilter(filters, Number(id)))}
          />
        ) : null}

        <FilterSection
          title="Created date"
          kind="multi"
          cap={OPTION_CAP}
          applied={createdApplied}
          options={dateOptions}
          onToggle={pickCreated}
          footer={
            customOpen ? (
              <div data-testid="project-views-custom-window" className="flex flex-col gap-(--space-2) p-(--space-2)">
                <DateRangePicker
                  label="Custom creation window"
                  mode="merged"
                  value={pendingCustom ?? filters.created.custom ?? { start: null, end: null }}
                  weekStart={weekStart}
                  onChange={applyCustom}
                  testId="project-views-custom-date"
                />
              </div>
            ) : undefined
          }
        />

        <FilterSection
          title="Created by"
          kind="multi"
          cap={OPTION_CAP}
          applied={filters.creators}
          options={creatorOptions}
          loading={members === undefined}
          loadingText="Loading people"
          onToggle={(id) => onFiltersChange(toggleViewCreatorFilter(filters, id))}
        />
      </div>
    </Popover>
  );
}
