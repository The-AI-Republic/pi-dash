// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake filter panel (parity/specs/intake.md INT-009): seven sections
// in fixed order on the shared section chrome, one search box over all
// of them, and the running applied count on the trigger. Status options
// follow the tab; priorities, members and labels are multi-select
// checkbox rows; the two date sections are single-select with an
// explicit-range dialog (INT-010). Every pick applies immediately and
// the panel stays open across picks.

import { Avatar, Button, Popover } from "@pidash/kit";
import { ListFilter, Search } from "lucide-react";
import * as React from "react";

import type { IntakeStatus } from "@pidash/api-client";

import { FilterSection } from "../../issues/index.js";
import { PRIORITY_ICONS, PRIORITY_TONES, selectPriorityOptions } from "../../../shared/pickers/index.js";
import { IntakeDateFilter } from "./IntakeDateFilter.js";
import type { IntakeDatePresetId, IntakeDateValue } from "../lib/dates.js";
import {
  appliedFilterCount,
  applyCustomDate,
  INTAKE_FILTER_SECTION_TITLES,
  pickDatePreset,
  toggleFilterId,
  toggleFilterStatus,
  type IntakeFilterSelection,
} from "../lib/filters.js";
import { INTAKE_STATUS_LABELS, statusOptionsForTab } from "../lib/statuses.js";
import type { IntakeMemberOption } from "../lib/members.js";
import type { IntakeTab } from "../lib/tabs.js";

export interface IntakeLabelOption {
  id: string;
  name: string;
  color: string;
}

export interface IntakeFilterPanelProps {
  tab: IntakeTab;
  selection: IntakeFilterSelection;
  assigneeOptions: readonly IntakeMemberOption[];
  createdByOptions: readonly IntakeMemberOption[];
  membersLoading: boolean;
  membersFailed: boolean;
  labelOptions: readonly IntakeLabelOption[];
  labelsLoading: boolean;
  labelsFailed: boolean;
  today: string;
  weekStart: number;
  onSelectionChange: (next: IntakeFilterSelection) => void;
}

function memberRow(option: IntakeMemberOption) {
  return {
    id: option.userId,
    label: option.displayName,
    icon: (
      <Avatar
        name={option.displayName}
        {...(option.avatarUrl === null ? {} : { src: option.avatarUrl })}
        size="small"
      />
    ),
    disabled: option.suspended,
  };
}

export function IntakeFilterPanel({
  tab,
  selection,
  assigneeOptions,
  createdByOptions,
  membersLoading,
  membersFailed,
  labelOptions,
  labelsLoading,
  labelsFailed,
  today,
  weekStart,
  onSelectionChange,
}: IntakeFilterPanelProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const [query, setQuery] = React.useState("");
  const count = appliedFilterCount(selection);

  const pick = (next: IntakeFilterSelection): void => onSelectionChange(next);
  const sharedSearch = { searchText: query, onSearchTextChange: setQuery, hideSearch: true as const };

  const memberError = membersFailed ? "Couldn't load members." : undefined;
  const labelError = labelsFailed ? "Couldn't load labels." : undefined;

  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      className="max-h-[26rem] w-80 overflow-y-auto"
      trigger={
        <Button
          variant="ghost"
          size="small"
          data-testid="intake-filters-trigger"
          aria-label={`Filters, ${count} applied`}
        >
          <ListFilter size={14} strokeWidth={2} aria-hidden="true" />
          <span>Filters</span>
          <span
            data-testid="intake-filters-count"
            aria-hidden="true"
            className="text-meta rounded-full bg-(--accent) px-(--space-2) font-medium text-(--on-accent)"
          >
            {count}
          </span>
        </Button>
      }
    >
      <div data-testid="intake-filters-panel" className="flex flex-col gap-(--space-2) p-(--space-2)">
        <label className="flex items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) px-(--space-2)">
          <Search size={12} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
          <input
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            aria-label="Search filters"
            placeholder="Search filters"
            className="text-body w-full bg-transparent py-(--space-1) text-(--text) outline-none placeholder:text-(--text-muted)"
          />
        </label>
        <FilterSection
          title={INTAKE_FILTER_SECTION_TITLES.status}
          kind="multi"
          applied={selection.statuses.map(String)}
          options={statusOptionsForTab(tab).map((status) => ({
            id: String(status),
            label: INTAKE_STATUS_LABELS[status],
          }))}
          onToggle={(id) => pick(toggleFilterStatus(selection, Number(id) as IntakeStatus))}
          testId="intake-filter-status"
          {...sharedSearch}
        />
        <FilterSection
          title={INTAKE_FILTER_SECTION_TITLES.priority}
          kind="multi"
          applied={[...selection.priorities]}
          options={selectPriorityOptions().map((option) => {
            const Glyph = PRIORITY_ICONS[option.level];
            return {
              id: option.level,
              label: option.label,
              icon: (
                <span aria-hidden="true" className={`inline-flex items-center ${PRIORITY_TONES[option.level]}`}>
                  <Glyph size={14} aria-hidden="true" />
                </span>
              ),
            };
          })}
          onToggle={(id) => pick(toggleFilterId(selection, "priorities", id))}
          testId="intake-filter-priority"
          {...sharedSearch}
        />
        <FilterSection
          title={INTAKE_FILTER_SECTION_TITLES.assignees}
          kind="multi"
          applied={[...selection.assignees]}
          options={assigneeOptions.map(memberRow)}
          onToggle={(id) => pick(toggleFilterId(selection, "assignees", id))}
          loading={membersLoading}
          errorText={memberError}
          testId="intake-filter-assignees"
          {...sharedSearch}
        />
        <FilterSection
          title={INTAKE_FILTER_SECTION_TITLES.createdBy}
          kind="multi"
          applied={[...selection.createdBy]}
          options={createdByOptions.map(memberRow)}
          onToggle={(id) => pick(toggleFilterId(selection, "createdBy", id))}
          loading={membersLoading}
          errorText={memberError}
          testId="intake-filter-createdBy"
          {...sharedSearch}
        />
        <FilterSection
          title={INTAKE_FILTER_SECTION_TITLES.labels}
          kind="multi"
          applied={[...selection.labels]}
          options={labelOptions.map((option) => ({
            id: option.id,
            label: option.name,
            icon: (
              <span
                aria-hidden="true"
                style={{ backgroundColor: option.color }}
                className="size-1.5 shrink-0 rounded-full"
              />
            ),
          }))}
          onToggle={(id) => pick(toggleFilterId(selection, "labels", id))}
          loading={labelsLoading}
          errorText={labelError}
          testId="intake-filter-labels"
          {...sharedSearch}
        />
        <IntakeDateFilter
          section="createdAt"
          value={selection.createdAt}
          searchText={query}
          onSearchTextChange={setQuery}
          today={today}
          weekStart={weekStart}
          onPresetToggle={(preset: IntakeDatePresetId) => pick(pickDatePreset(selection, "createdAt", preset))}
          onCustomApply={(value: IntakeDateValue) => pick(applyCustomDate(selection, "createdAt", value))}
        />
        <IntakeDateFilter
          section="updatedAt"
          value={selection.updatedAt}
          searchText={query}
          onSearchTextChange={setQuery}
          today={today}
          weekStart={weekStart}
          onPresetToggle={(preset: IntakeDatePresetId) => pick(pickDatePreset(selection, "updatedAt", preset))}
          onCustomApply={(value: IntakeDateValue) => pick(applyCustomDate(selection, "updatedAt", value))}
        />
      </div>
    </Popover>
  );
}
