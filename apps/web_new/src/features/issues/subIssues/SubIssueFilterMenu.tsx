// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sub-issue filter dropdown (parity/specs/issues.md §15, ISS-178, with
// the section chrome of §22 ISS-094). Priority, state group, state,
// project, assignees and the two date dimensions, each a collapsible
// section with search and the five-row cap. A date section keeps its
// presets above and its Custom day below the rows, in the same control
// the legacy filters use. Values apply to the loaded page only and are
// held per parent by the store.

import { Button, Popover } from "@pidash/kit";
import { ListFilter } from "lucide-react";
import * as React from "react";

import {
  DATE_PRESETS,
  customEndEntry,
  customStartEntry,
  hasCustomDate,
  toggleDatePreset,
} from "../filters/rich/legacyDimensions.js";
import { selectedIds } from "../filters/rich/filterValues.js";
import { FilterSection, type FilterSectionOption } from "../filters/shared/FilterSection.js";
import { EditionAppliedChips, EditionFilterSections } from "../edition/EditionFilterSections.js";
import { editionFilterCount, type EditionChip, type EditionFilters } from "../edition/filterModel.js";
import { usePickerProjects, usePickerStates } from "../../../shared/pickers/reference.js";
import {
  PRIORITY_ICONS,
  PRIORITY_LABELS,
  PRIORITY_LEVELS,
  PRIORITY_TONES,
} from "../../../shared/pickers/priorities.js";
import { STATE_GROUP_ICONS, type StateGroup } from "../../../shared/pickers/states.js";
import {
  appliedFilterCount,
  SUB_ISSUE_FILTER_KEYS,
  SUB_ISSUE_FILTER_LABELS,
  type SubIssueFilterKey,
  type SubIssueFilters,
} from "./rules.js";
import type { SubIssueFilterSources } from "./types.js";

/** The system groups offered, in workflow order. */
export const SUB_ISSUE_STATE_GROUPS: readonly StateGroup[] = [
  "backlog",
  "unstarted",
  "started",
  "review",
  "test",
  "completed",
  "cancelled",
];

/** An `aria-label`-free, human label for a group. */
export function stateGroupLabel(group: StateGroup): string {
  return group.charAt(0).toUpperCase() + group.slice(1);
}

export interface SubIssueFilterMenuProps {
  workspaceSlug: string;
  projectId: string;
  filters: SubIssueFilters;
  sources: SubIssueFilterSources;
  /** Which dimensions this parent offers (project only for cross-project children). */
  offered?: readonly SubIssueFilterKey[] | undefined;
  onToggle: (key: SubIssueFilterKey, value: string) => void;
  /** Replace the date dimension's entries (presets plus custom). */
  onSetDate: (key: "start_date" | "target_date", values: readonly string[]) => void;
  /**
   * Edition-only dimensions (team-project scoping and work-item type,
   * ISS-234). Absent keeps the community dropdown exactly as it was: no
   * extra section and no chip row.
   */
  editionFilters?: EditionFilters | undefined;
  onToggleEdition?: ((key: keyof EditionFilters, value: string) => void) | undefined;
  editionChips?: readonly EditionChip[] | undefined;
  onRemoveEditionChip?: ((chip: EditionChip) => void) | undefined;
}

function priorityRows(): FilterSectionOption[] {
  // Later levels first so urgent sits on top of the list.
  return [...PRIORITY_LEVELS].reverse().map((level) => {
    const Icon = PRIORITY_ICONS[level];
    return {
      id: level,
      label: PRIORITY_LABELS[level],
      searchText: level,
      icon: (
        <span className={`inline-flex items-center ${PRIORITY_TONES[level]}`}>
          <Icon size={12} strokeWidth={2} />
        </span>
      ),
    };
  });
}

function stateGroupRows(): FilterSectionOption[] {
  return SUB_ISSUE_STATE_GROUPS.map((group) => {
    const Icon = STATE_GROUP_ICONS[group];
    return {
      id: group,
      label: stateGroupLabel(group),
      searchText: group,
      icon: <Icon size={12} strokeWidth={1.5} />,
    };
  });
}

/** A stored custom entry back to its day and direction. */
function customParts(entries: readonly string[]): { day: string; direction: "on" | "after" | "before" } {
  const entry = entries[0];
  if (entry === undefined) return { day: "", direction: "on" };
  const [day = "", direction] = entry.split(";");
  if (direction === "after") return { day, direction: "after" };
  if (direction === "before") return { day, direction: "before" };
  return { day, direction: "on" };
}

/** One date dimension: presets, then a Custom day below the rows. */
function DateSection({
  dimension,
  filters,
  onChange,
}: {
  dimension: "start_date" | "target_date";
  filters: SubIssueFilters;
  onChange: (values: readonly string[]) => void;
}): React.ReactElement {
  const value = filters[dimension];
  const presets = value.filter((entry) => DATE_PRESETS.some((preset) => preset.value === entry));
  const custom = value.filter((entry) => !DATE_PRESETS.some((preset) => preset.value === entry));
  const direction = customParts(custom).direction;
  const day = customParts(custom).day;
  const options: FilterSectionOption[] = DATE_PRESETS.map((preset) => ({
    id: preset.value,
    label: preset.label,
  }));

  function setCustom(nextDay: string, nextDirection: "on" | "after" | "before"): void {
    const entry =
      nextDirection === "on"
        ? nextDay
        : nextDirection === "after"
          ? customStartEntry(nextDay)
          : customEndEntry(nextDay);
    onChange([...presets, entry]);
  }

  const footer = (
    <div className="flex flex-col gap-(--space-2) p-(--space-2)">
      <label className="flex items-center gap-(--space-2)">
        <span className="text-caption text-(--text-muted)">Custom</span>
        <input
          type="date"
          aria-label={`${SUB_ISSUE_FILTER_LABELS[dimension]} custom day`}
          value={day}
          onChange={(event) => {
            const next = event.target.value;
            if (next === "") onChange(presets);
            else setCustom(next, direction === "on" ? "on" : direction);
          }}
          className="text-body rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) py-(--space-1) text-(--text)"
        />
      </label>
      <div
        role="radiogroup"
        aria-label={`${SUB_ISSUE_FILTER_LABELS[dimension]} custom direction`}
        className="flex gap-(--space-1)"
      >
        {(["on", "after", "before"] as const).map((option) => (
          <button
            key={option}
            type="button"
            role="radio"
            aria-checked={direction === option}
            onClick={() => (day === "" ? undefined : setCustom(day, option))}
            className={`text-caption rounded-(--radius-control) border px-(--space-2) py-px ${
              direction === option
                ? "border-(--accent) bg-(--accent) text-(--on-accent)"
                : "border-(--border) text-(--text-muted)"
            }`}
          >
            {option === "on" ? "On" : option === "after" ? "After" : "Before"}
          </button>
        ))}
        <button
          type="button"
          aria-label={`Clear ${SUB_ISSUE_FILTER_LABELS[dimension]} custom day`}
          onClick={() => onChange(presets)}
          className="text-caption rounded-(--radius-control) px-(--space-2) py-px text-(--text-muted) hover:bg-(--subtle)"
        >
          Clear
        </button>
      </div>
    </div>
  );

  return (
    <FilterSection
      title={SUB_ISSUE_FILTER_LABELS[dimension]}
      kind="multi"
      applied={value}
      options={options}
      onToggle={(entry) => onChange(selectedIds(toggleDatePreset(value, entry)))}
      footer={footer}
      testId={`sub-issue-filter-${dimension}`}
      className="py-(--space-2)"
    />
  );
}

export function SubIssueFilterMenu({
  workspaceSlug,
  projectId,
  filters,
  sources,
  offered = SUB_ISSUE_FILTER_KEYS,
  onToggle,
  onSetDate,
  editionFilters,
  onToggleEdition,
  editionChips,
  onRemoveEditionChip,
}: SubIssueFilterMenuProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const states = usePickerStates(workspaceSlug, projectId, open && offered.includes("state"));
  const projects = usePickerProjects(workspaceSlug, open && offered.includes("project"));
  // The edition dimensions count towards the trigger's applied total: the
  // old rich-filter button counted every active dimension it rendered
  // (ISS-234). Absent edition filters leave the community total unchanged.
  const count = appliedFilterCount(filters) + (editionFilters === undefined ? 0 : editionFilterCount(editionFilters));

  const stateRows: FilterSectionOption[] = (states.data ?? []).map((state) => {
    const Icon = STATE_GROUP_ICONS[state.group];
    return {
      id: state.id,
      label: state.name,
      searchText: `${state.name} ${state.group}`,
      icon: <Icon size={12} strokeWidth={1.5} style={{ color: state.color }} />,
    };
  });
  const projectRows: FilterSectionOption[] = (projects.data ?? []).map((project) => ({
    id: project.id,
    label: project.name,
    searchText: `${project.name} ${project.identifier}`,
  }));

  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      className="max-h-[26rem] w-80 overflow-y-auto"
      trigger={
        <Button
          variant="ghost"
          size="small"
          data-testid="sub-issue-filters-trigger"
          aria-label={count > 0 ? `Filters, ${count} applied` : "Filters"}
          aria-pressed={count > 0}
        >
          <ListFilter size={14} strokeWidth={2} aria-hidden="true" />
        </Button>
      }
    >
      <div className="divide-y divide-(--border)" data-testid="sub-issue-filters-panel">
        {editionChips === undefined || editionChips.length === 0 || onRemoveEditionChip === undefined ? null : (
          <div className="p-(--space-2)">
            <EditionAppliedChips chips={editionChips} onRemove={onRemoveEditionChip} />
          </div>
        )}
        <FilterSection
          title={SUB_ISSUE_FILTER_LABELS.priority}
          kind="multi"
          applied={filters.priority}
          options={priorityRows()}
          onToggle={(value) => onToggle("priority", value)}
          testId="sub-issue-filter-priority"
          className="py-(--space-2)"
        />
        <FilterSection
          title={SUB_ISSUE_FILTER_LABELS.state_group}
          kind="multi"
          applied={filters.state_group}
          options={stateGroupRows()}
          onToggle={(value) => onToggle("state_group", value)}
          testId="sub-issue-filter-state-group"
          className="py-(--space-2)"
        />
        {offered.includes("state") ? (
          <FilterSection
            title={SUB_ISSUE_FILTER_LABELS.state}
            kind="multi"
            applied={filters.state}
            options={stateRows}
            onToggle={(value) => onToggle("state", value)}
            loading={open && states.isPending}
            loadingText="Loading states"
            errorText={states.isError ? "Couldn't load states." : undefined}
            testId="sub-issue-filter-state"
            className="py-(--space-2)"
          />
        ) : null}
        {offered.includes("project") ? (
          <FilterSection
            title={SUB_ISSUE_FILTER_LABELS.project}
            kind="multi"
            applied={filters.project}
            options={projectRows}
            onToggle={(value) => onToggle("project", value)}
            loading={open && projects.isPending}
            loadingText="Loading projects"
            errorText={projects.isError ? "Couldn't load projects." : undefined}
            testId="sub-issue-filter-project"
            className="py-(--space-2)"
          />
        ) : null}
        {offered.includes("assignees") ? (
          <FilterSection
            title={SUB_ISSUE_FILTER_LABELS.assignees}
            kind="multi"
            applied={filters.assignees}
            options={sources.assigneeRows}
            onToggle={(value) => onToggle("assignees", value)}
            loading={sources.assigneesLoading}
            loadingText="Loading members"
            testId="sub-issue-filter-assignees"
            className="py-(--space-2)"
          />
        ) : null}
        {editionFilters === undefined || onToggleEdition === undefined ? null : (
          <EditionFilterSections
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            open={open}
            filters={editionFilters}
            onToggle={onToggleEdition}
          />
        )}
        {offered.includes("start_date") ? (
          <DateSection
            dimension="start_date"
            filters={filters}
            onChange={(values) => onSetDate("start_date", values)}
          />
        ) : null}
        {offered.includes("target_date") ? (
          <DateSection
            dimension="target_date"
            filters={filters}
            onChange={(values) => onSetDate("target_date", values)}
          />
        ) : null}
      </div>
    </Popover>
  );
}

/** Whether a date section holds a custom day (drives its tick). */
export function hasCustomDay(filters: SubIssueFilters, dimension: "start_date" | "target_date"): boolean {
  return hasCustomDate(filters[dimension]);
}
