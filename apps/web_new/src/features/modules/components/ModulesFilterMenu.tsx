// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Filters menu for the modules list (parity/specs/modules.md §2.6,
// MOD-006). One popover with its own option search (which narrows the
// offered values, never the rows) and the groups in spec order: the
// Favorites switch, the six lifecycle statuses, Lead and Members (the
// signed-in member first, five at a time with a view-all expander) and
// the Start date and Due date dimensions. Relative date choices are lower
// bounds; Custom opens the shared range calendar whose default window is
// today through the same day next month. Tokens inside a date dimension
// are AND-ed, so the dialog adds its two tokens to the relative ones. The
// archived screen mounts this menu with favorites and status hidden, so
// those are props, never a second copy.

import { Avatar, Button, Popover } from "@pidash/kit";
import type { ModuleStatus, WorkspaceMember } from "@pidash/api-client";
import { ListFilter, Search, X } from "lucide-react";
import * as React from "react";

import {
  DateRangeCalendar,
  FilterSection,
  useWeekStart,
  type FilterSectionOption,
  type RangeValue,
} from "../../issues/index.js";
import {
  clearModuleDateRange,
  defaultModuleCustomRange,
  emptyModuleFilters,
  hasModuleFilters,
  matchesModuleMenuQuery,
  MODULE_DATE_CUSTOM_LABEL,
  MODULE_DATE_PRESETS,
  MODULE_FILTER_LABELS,
  MODULE_STATUS_OPTIONS,
  moduleDateRange,
  moduleCustomWindow,
  moduleDateTokenKey,
  selectModuleMemberOptions,
  toggleModuleDateToken,
  toggleModuleFilterValue,
  type ModuleListFilters,
  type ModuleDatePreset,
} from "../lib/moduleListControls.js";

/** People shown per group before the view-all expander (§2.6). */
export const MODULE_FILTER_OPTION_CAP = 5;

/** The six lifecycle states always render together (§2.6). */
export const MODULE_STATUS_OPTION_CAP = 6;

type DateDimension = "start-date" | "due-date";
type PeopleDimension = "lead" | "members";

export interface ModulesFilterMenuProps {
  filters: ModuleListFilters;
  /** Workspace members; undefined while the directory resolves. */
  members: readonly WorkspaceMember[] | undefined;
  /** The signed-in user, offered as "You" first. */
  meId: string | null;
  onFiltersChange: (filters: ModuleListFilters) => void;
  /** Archived list hides the favorites dimension (§7.3). */
  showFavorites?: boolean | undefined;
  /** Archived list hides the status dimension (§7.3). */
  showStatus?: boolean | undefined;
  /** Controlled panel state, so a host can close it. */
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
}

function TotalStatus({ count }: { count: number }): React.ReactElement {
  return (
    <span
      data-testid="modules-filter-dimension-count"
      className="text-meta rounded-(--radius-full) bg-(--subtle) px-(--space-2) text-(--text-muted)"
    >
      {count}
    </span>
  );
}

function PeopleSection({
  title,
  dimension,
  filters,
  members,
  meId,
  query,
  onFiltersChange,
}: {
  title: string;
  dimension: PeopleDimension;
  filters: ModuleListFilters;
  members: readonly WorkspaceMember[] | undefined;
  meId: string | null;
  query: string;
  onFiltersChange: (filters: ModuleListFilters) => void;
}): React.ReactElement {
  const [expanded, setExpanded] = React.useState(false);
  const options = selectModuleMemberOptions({ members, meId, selectedIds: filters[dimension] });
  const matching = options.filter((option) => matchesModuleMenuQuery(option.label, option.searchText, query));
  const visible = expanded ? matching : matching.slice(0, MODULE_FILTER_OPTION_CAP);

  return (
    <section data-testid="modules-filter-group" data-filter-group={title} aria-label={title}>
      <div className="flex items-center justify-between gap-(--space-2)">
        <div className="flex items-center gap-(--space-2)">
          <h3 className="text-caption text-(--text-muted)" title={title}>
            {title}
          </h3>
          {filters[dimension].length > 0 ? <TotalStatus count={filters[dimension].length} /> : null}
        </div>
      </div>
      <div className="mt-(--space-1)">
        {matching.length === 0 ? (
          <p className="text-caption px-(--space-3) py-(--space-2) text-(--text-muted) italic">No matches found</p>
        ) : (
          visible.map((option) => (
            <button
              key={option.userId}
              type="button"
              role="checkbox"
              aria-checked={filters[dimension].includes(option.userId)}
              aria-label={option.label}
              data-filter-option={option.label}
              onClick={() => onFiltersChange(toggleModuleFilterValue(filters, dimension, option.userId))}
              className="flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-left hover:bg-(--subtle)"
            >
              <span
                aria-hidden="true"
                className={`grid size-3.5 shrink-0 place-items-center rounded border ${
                  filters[dimension].includes(option.userId)
                    ? "border-(--accent) bg-(--accent) text-(--on-accent)"
                    : "border-(--border)"
                }`}
              >
                {filters[dimension].includes(option.userId) ? (
                  <span className="size-1.5 rounded-full bg-(--on-accent)" />
                ) : null}
              </span>
              <span aria-hidden="true">
                {option.avatarUrl === null ? null : <Avatar name={option.label} src={option.avatarUrl} size="small" />}
              </span>
              <span className="truncate" title={option.searchText}>
                {option.label}
              </span>
            </button>
          ))
        )}
        {matching.length > MODULE_FILTER_OPTION_CAP ? (
          <button
            type="button"
            onClick={() => setExpanded(!expanded)}
            className="text-caption px-(--space-3) py-(--space-1) font-medium text-(--accent)"
          >
            {expanded ? "View less" : "View all"}
          </button>
        ) : null}
      </div>
    </section>
  );
}

function DateSection({
  title,
  dimension,
  filters,
  weekStart,
  query,
  onFiltersChange,
}: {
  title: string;
  dimension: DateDimension;
  filters: ModuleListFilters;
  weekStart: number;
  query: string;
  onFiltersChange: (filters: ModuleListFilters) => void;
}): React.ReactElement {
  const [customOpen, setCustomOpen] = React.useState(false);
  // The window picked in the open dialog. Until the first pick the calendar
  // shows the documented default (today through the same day next month,
  // §2.6.1); Apply writes the draft when there is one and the default
  // otherwise, so nothing reaches the list before Apply.
  const [draftRange, setDraftRange] = React.useState<RangeValue | null>(null);
  const range = moduleDateRange(filters, dimension);
  const options: FilterSectionOption[] = MODULE_DATE_PRESETS.map((preset) => ({
    id: preset.value,
    label: preset.label,
  })).filter((option) => matchesModuleMenuQuery(option.label, "", query));
  const customMatches = matchesModuleMenuQuery(MODULE_DATE_CUSTOM_LABEL, "", query);
  const applied = filters[dimension].map(moduleDateTokenKey);

  const pickPreset = (preset: ModuleDatePreset): void => {
    onFiltersChange(toggleModuleDateToken(filters, dimension, { kind: "preset", value: preset }));
  };

  const pickCustom = (): void => {
    // The Custom entry toggles: with a range applied it clears the range and
    // keeps relative tokens; without one it reveals the calendar.
    if (range !== null) {
      onFiltersChange(clearModuleDateRange(filters, dimension));
      setCustomOpen(false);
      setDraftRange(null);
      return;
    }
    setDraftRange(null);
    setCustomOpen(true);
  };

  const applyRange = (): void => {
    // Applying always writes a window: the picks, or the default the
    // calendar showed when it was not touched (§2.6.1).
    setCustomOpen(false);
    setDraftRange(null);
    onFiltersChange(
      toggleModuleDateToken(filters, dimension, { kind: "range", value: moduleCustomWindow(draftRange) })
    );
  };

  return (
    <div data-testid="modules-filter-group" data-filter-group={title}>
      <FilterSection
        title={title}
        kind="multi"
        cap={MODULE_FILTER_OPTION_CAP}
        applied={applied}
        options={options}
        onToggle={(id) => pickPreset(id as ModuleDatePreset)}
        testId="modules-filter-date-group"
        footer={
          customMatches ? (
            <div className="flex flex-col gap-(--space-2) p-(--space-2)">
              <button
                type="button"
                role="checkbox"
                aria-checked={range !== null}
                aria-label={MODULE_DATE_CUSTOM_LABEL}
                data-filter-option={MODULE_DATE_CUSTOM_LABEL}
                onClick={pickCustom}
                className="flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-left hover:bg-(--subtle)"
              >
                <span
                  aria-hidden="true"
                  className={`grid size-3.5 shrink-0 place-items-center rounded border ${
                    range !== null ? "border-(--accent) bg-(--accent)" : "border-(--border)"
                  }`}
                >
                  {range !== null ? <span className="size-1.5 rounded-full bg-(--on-accent)" /> : null}
                </span>
                <span className="truncate" title={MODULE_DATE_CUSTOM_LABEL}>
                  {MODULE_DATE_CUSTOM_LABEL}
                </span>
              </button>
              {customOpen ? (
                <div data-testid="modules-custom-date" className="flex flex-col gap-(--space-2)">
                  <DateRangeCalendar
                    // An applied range is toggled off before the calendar
                    // opens, so this is either the draft being picked or the
                    // documented default window.
                    value={draftRange ?? defaultModuleCustomRange()}
                    onChange={setDraftRange}
                    weekStart={weekStart}
                    testId="modules-custom-calendar"
                  />
                  <div className="flex justify-end gap-(--space-2)">
                    <Button
                      variant="ghost"
                      size="small"
                      onClick={() => {
                        setDraftRange(null);
                        setCustomOpen(false);
                      }}
                    >
                      Cancel
                    </Button>
                    <Button variant="primary" size="small" data-testid="modules-custom-apply" onClick={applyRange}>
                      Apply
                    </Button>
                  </div>
                </div>
              ) : null}
            </div>
          ) : undefined
        }
      />
    </div>
  );
}

/** One status option with its color dot. */
function statusOptions(query: string): FilterSectionOption[] {
  return MODULE_STATUS_OPTIONS.filter((status) => matchesModuleMenuQuery(status.label, "", query)).map((status) => ({
    id: status.value,
    label: status.label,
    icon: (
      <span
        data-testid="modules-status-dot"
        data-status={status.value}
        style={{ backgroundColor: status.color }}
        className="size-2 rounded-full"
      />
    ),
  }));
}

export function ModulesFilterMenu({
  filters,
  members,
  meId,
  onFiltersChange,
  showFavorites = true,
  showStatus = true,
  open,
  onOpenChange,
}: ModulesFilterMenuProps): React.ReactElement {
  const weekStart = useWeekStart();
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(false);
  const panelOpen = open ?? uncontrolledOpen;
  const [optionSearch, setOptionSearch] = React.useState("");
  const active = hasModuleFilters(filters);

  function setOpen(next: boolean): void {
    if (open === undefined) setUncontrolledOpen(next);
    onOpenChange?.(next);
    if (!next) setOptionSearch("");
  }

  const favoritesMatch = matchesModuleMenuQuery(MODULE_FILTER_LABELS.favorites, "", optionSearch);
  const statuses = statusOptions(optionSearch);

  return (
    <Popover
      open={panelOpen}
      onOpenChange={setOpen}
      trigger={
        <Button variant="secondary" size="small" data-testid="modules-filter-trigger">
          <ListFilter size={14} strokeWidth={2} aria-hidden="true" />
          <span>Filters</span>
          {active ? (
            <span
              data-testid="modules-filter-active"
              aria-hidden="true"
              className="size-1.5 rounded-full bg-(--accent)"
            />
          ) : null}
        </Button>
      }
      className="max-h-[28rem] w-80 overflow-y-auto"
    >
      <div data-testid="modules-filter-panel" className="flex flex-col gap-(--space-2)">
        <div className="flex items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) py-(--space-2)">
          <Search size={14} strokeWidth={2} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
          <input
            type="text"
            aria-label="Search filters"
            placeholder="Search"
            value={optionSearch}
            data-testid="modules-filter-search"
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

        {showFavorites && favoritesMatch ? (
          <label
            data-testid="modules-filter-favorites"
            className="text-body flex cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) hover:bg-(--subtle)"
          >
            <input
              type="checkbox"
              checked={filters.favorites}
              onChange={() => onFiltersChange(toggleModuleFilterValue(filters, "favorites"))}
            />
            <span className="truncate" title={MODULE_FILTER_LABELS.favorites}>
              {MODULE_FILTER_LABELS.favorites}
            </span>
          </label>
        ) : null}

        {showStatus && statuses.length > 0 ? (
          <div data-testid="modules-filter-group" data-filter-group={MODULE_FILTER_LABELS.status}>
            <FilterSection
              title={MODULE_FILTER_LABELS.status}
              kind="multi"
              cap={MODULE_STATUS_OPTION_CAP}
              applied={filters.status}
              count={filters.status.length}
              options={statuses}
              onToggle={(id) => onFiltersChange(toggleModuleFilterValue(filters, "status", id as ModuleStatus))}
              testId="modules-filter-status"
            />
          </div>
        ) : null}

        <PeopleSection
          title={MODULE_FILTER_LABELS.lead}
          dimension="lead"
          filters={filters}
          members={members}
          meId={meId}
          query={optionSearch}
          onFiltersChange={onFiltersChange}
        />
        <PeopleSection
          title={MODULE_FILTER_LABELS.members}
          dimension="members"
          filters={filters}
          members={members}
          meId={meId}
          query={optionSearch}
          onFiltersChange={onFiltersChange}
        />
        <DateSection
          title={MODULE_FILTER_LABELS["start-date"]}
          dimension="start-date"
          filters={filters}
          weekStart={weekStart}
          query={optionSearch}
          onFiltersChange={onFiltersChange}
        />
        <DateSection
          title={MODULE_FILTER_LABELS["due-date"]}
          dimension="due-date"
          filters={filters}
          weekStart={weekStart}
          query={optionSearch}
          onFiltersChange={onFiltersChange}
        />
      </div>
    </Popover>
  );
}

/** Re-exported for hosts that reset from a menu action. */
export { emptyModuleFilters };
