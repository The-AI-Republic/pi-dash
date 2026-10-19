// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Rich-filter value slot (parity/specs/issues.md §9, ISS-082–105).
// The slot adapts to the field's source: workflow states with their group
// glyph, the fixed state-group and priority sets, project members with
// avatars for assignee/creator/mentions, and project labels with colored
// dots. Multi-select slots stay open across picks and clear a re-picked
// value, reading as the first two names plus an overflow count (ISS-101).
// Date kinds get bounded calendars: one final day (ISS-102), or two ends
// committed only as a complete pair (ISS-103). Long lists read back at the
// shared five-row cap with View all, and every section's rows share the
// same applied-first ordering (ISS-089–091, ISS-094).

import { Avatar, Button, Combobox, Popover, type ComboboxOption } from "@pidash/kit";
import { CalendarDays, Check, ChevronDown, Circle, CircleCheck, CircleDashed, CircleDot, Package } from "lucide-react";
import * as React from "react";

import { useMe } from "../../../../core/session/hooks.js";
import {
  PRIORITY_ICONS,
  StateGlyph,
  STATE_GROUP_ICONS,
  selectCycleOptions,
  selectMemberOptions,
  selectModuleOptions,
  selectProjectMark,
  selectProjectOptions,
  selectStateOptions,
  usePickerCycles,
  usePickerModules,
  usePickerProjectMembers,
  usePickerProjects,
  usePickerStates,
  usePickerWorkspaceMembers,
  type StateGroup,
} from "../../../../shared/pickers/index.js";
import { useLabels } from "../../labels/api.js";
import { DateCalendar } from "../../dropdowns/DatePicker.js";
import { DateRangeCalendar } from "../../dropdowns/DateRangePicker.js";
import { useWeekStart } from "../../dropdowns/weekStart.js";
import type { RangeValue } from "../../dropdowns/calendar.js";
import type { RichCondition, RichFilterField } from "./expression.js";
import {
  DATE_PRESETS,
  clearCustomDate,
  composeDateValue,
  customDays,
  customRange,
  dateEntries,
  dateValueLabel,
  dateValueParts,
  hasCustomDate,
  cycleLifecycle,
  toggleDatePreset,
  type CycleLifecycle,
} from "./legacyDimensions.js";
import { orderAppliedByName, orderLabelOptions, orderMemberOptions, selectedIds } from "./filterValues.js";
import { FILTER_SECTION_CAP } from "../shared/FilterSection.js";
import {
  EMPTY_SLOT_LABEL,
  isRangeIncomplete,
  opensWhenIncomplete,
  pickTriggerLabel,
  rangeLabel,
  slotEditor,
  togglePick as togglePickValue,
} from "./slotModel.js";
import { isValueCompleteForOperator } from "./expression.js";

export interface ValueSlotProps {
  workspaceSlug: string;
  projectId: string;
  field: RichFilterField;
  condition: RichCondition;
  onChange: (value: unknown) => void;
}

/** One lifecycle glyph; the running cycle pulses (ISS-089). */
function CycleLifecycleGlyph({ lifecycle }: { lifecycle: CycleLifecycle }): React.ReactElement {
  const running = lifecycle === "current";
  const common = { size: 14, strokeWidth: 1.5, "aria-hidden": true } as const;
  const glyph =
    lifecycle === "completed" ? (
      <CircleCheck {...common} />
    ) : lifecycle === "upcoming" ? (
      <CircleDashed {...common} />
    ) : lifecycle === "current" ? (
      <CircleDot {...common} />
    ) : (
      <Circle {...common} />
    );
  return (
    <span
      data-testid={`cycle-glyph-${lifecycle}`}
      data-running={running ? "true" : "false"}
      className={`inline-flex shrink-0 items-center ${running ? "animate-pulse text-(--accent)" : "text-(--text-muted)"}`}
    >
      {glyph}
    </span>
  );
}

/** One row of a date dimension's preset list. */
function PresetRow({
  checked,
  label,
  onToggle,
}: {
  checked: boolean;
  label: string;
  onToggle: () => void;
}): React.ReactElement {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      aria-label={label}
      onClick={onToggle}
      className="flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-left hover:bg-(--subtle)"
    >
      <span
        aria-hidden="true"
        className={`grid size-3.5 shrink-0 place-items-center rounded border ${
          checked ? "border-(--accent) bg-(--accent) text-(--on-accent)" : "border-(--border)"
        }`}
      >
        {checked ? <Check size={10} strokeWidth={3} /> : null}
      </span>
      <span className="text-body truncate text-(--text)">{label}</span>
    </button>
  );
}

/**
 * The relative windows a date dimension offers above its calendar
 * (ISS-092). A preset toggles on and off without touching the custom day;
 * a preset alone completes the condition, and the calendar below still
 * takes a custom day when the user wants one.
 */
function PresetList({ value, onChange }: { value: unknown; onChange: (value: unknown) => void }): React.ReactElement {
  return (
    <div className="flex w-56 flex-col gap-(--space-1) border-b border-(--border) p-(--space-2) pb-(--space-3)">
      <span className="text-caption px-(--space-3) text-(--text-muted)">Relative</span>
      {DATE_PRESETS.map((preset) => (
        <PresetRow
          key={preset.value}
          label={preset.label}
          checked={dateValueParts(value).presets.includes(preset.value)}
          onToggle={() => onChange(toggleDatePreset(value, preset.value))}
        />
      ))}
      <span className="text-caption px-(--space-3) pt-(--space-1) text-(--text-muted)">Custom</span>
      <PresetRow label="Custom" checked={hasCustomDate(value)} onToggle={() => onChange(clearCustomDate(value))} />
    </div>
  );
}

/** Selected values as a string array; a lone stored value reads as one pick. */
export function selectedValues(value: unknown): string[] {
  return selectedIds(value);
}

function chosenLabel(values: readonly string[], labels: ReadonlyMap<string, string>): string {
  return pickTriggerLabel(values.map((value) => labels.get(value) ?? value));
}

function PickSlot({
  workspaceSlug,
  projectId,
  field,
  condition,
  onChange,
  autoOpen,
}: ValueSlotProps & { autoOpen: boolean }): React.ReactElement {
  // A freshly added field opens its list by itself so the first pick is one
  // click away (ISS-100/ISS-101); a slot that already holds values stays
  // closed until its trigger is used.
  const [open, setOpen] = React.useState(autoOpen);
  const selected = selectedValues(condition.value);
  // A stored condition must resolve its names before the popup opens, so
  // reads stay enabled while ids are selected as well as while open.
  const readValues = open || selected.length > 0;
  const states = usePickerStates(workspaceSlug, projectId, field.source === "states" && readValues);
  const members = usePickerProjectMembers(workspaceSlug, projectId, field.source === "members" && readValues);
  const directory = usePickerWorkspaceMembers(workspaceSlug, field.source === "members" && readValues);
  const labels = useLabels(workspaceSlug, projectId, field.source === "labels" && readValues);
  // Project, cycle and module picks borrow the shared pickers' thin reads;
  // all three stay lazy until opened or while resolving stored ids.
  const projects = usePickerProjects(workspaceSlug, field.source === "projects" && readValues);
  const cycles = usePickerCycles(workspaceSlug, projectId, field.source === "cycles" && readValues);
  const modules = usePickerModules(workspaceSlug, projectId, field.source === "modules" && readValues);
  const me = useMe();
  const meId = me.data?.id ?? null;

  // Applied states lead and the rest keep server order (ISS-082).
  const stateOptions = React.useMemo(() => {
    if (field.source !== "states") return [];
    const chosenSet = new Set(selected);
    const options = selectStateOptions(states.data ?? []);
    return [
      ...options.filter((option) => chosenSet.has(option.id)),
      ...options.filter((option) => !chosenSet.has(option.id)),
    ];
  }, [field.source, selected, states.data]);

  const memberOptions = React.useMemo(() => {
    if (field.source !== "members" || directory.data === undefined || members.data === undefined) return [];
    return orderMemberOptions(
      selectMemberOptions({
        projectMembers: members.data,
        directory: directory.data,
        meId,
        selectedIds: selected,
      }),
      selected,
      meId
    );
  }, [directory.data, field.source, meId, members.data, selected]);

  const labelOptions = React.useMemo(
    () => (field.source === "labels" ? orderLabelOptions(labels.data ?? [], selected) : []),
    [field.source, labels.data, selected]
  );

  // Server order with applied rows first for projects and modules; the
  // cycle list drops completed cycles and the No-cycle clear row, which
  // has no id to filter on.
  // Applied picks lead, then names alphabetically (ISS-089–091). The
  // cycle list drops completed cycles and the No-cycle clear row, which
  // has no id to filter on.
  const projectOptions = React.useMemo(
    () =>
      field.source === "projects"
        ? orderAppliedByName(selectProjectOptions({ projects: projects.data ?? [], selectedIds: [] }), selected)
        : [],
    [field.source, projects.data, selected]
  );
  const cycleOptions = React.useMemo(
    () =>
      field.source === "cycles"
        ? orderAppliedByName(selectCycleOptions({ cycles: cycles.data ?? [], allowClear: false }), selected)
        : [],
    [cycles.data, field.source, selected]
  );
  const moduleOptions = React.useMemo(
    () =>
      field.source === "modules"
        ? orderAppliedByName(
            selectModuleOptions({ modules: modules.data ?? [], selectedIds: [], single: false }),
            selected
          )
        : [],
    [field.source, modules.data, selected]
  );
  // Cycle rows keep the status the option list dropped; the running one
  // pulses and each lifecycle gets its own glyph.
  const cycleStatuses = React.useMemo(() => {
    const map = new Map<string, string | undefined>();
    for (const cycle of cycles.data ?? []) map.set(cycle.id, cycle.status);
    return map;
  }, [cycles.data]);

  const labelsById = React.useMemo(() => {
    const map = new Map<string, string>();
    for (const state of stateOptions) map.set(state.id, state.name);
    for (const member of memberOptions) map.set(member.userId, member.label);
    for (const label of labelOptions) map.set(label.id, label.name);
    for (const project of projectOptions) map.set(project.id, project.name);
    for (const cycle of cycleOptions) map.set(cycle.id, cycle.name);
    for (const module of moduleOptions) map.set(module.id, module.name);
    for (const option of field.staticValues ?? []) map.set(option.value, option.label);
    return map;
  }, [cycleOptions, field.staticValues, labelOptions, memberOptions, moduleOptions, projectOptions, stateOptions]);

  const options: ComboboxOption[] = React.useMemo(() => {
    if (field.source === "states") {
      return stateOptions.map((option) => ({
        id: option.id,
        label: option.name,
        searchText: `${option.name} ${option.group}`,
        icon: <StateGlyph option={option} />,
      }));
    }
    if (field.source === "members") {
      return memberOptions.map((option) => ({
        id: option.userId,
        label: option.label,
        searchText: option.searchText,
        disabled: option.suspended,
        icon: (
          <Avatar
            name={option.searchText}
            {...(option.avatarUrl === null ? {} : { src: option.avatarUrl })}
            size="small"
          />
        ),
      }));
    }
    if (field.source === "labels") {
      return labelOptions.map((label) => ({
        id: label.id,
        label: label.name,
        icon: (
          <span
            aria-hidden="true"
            className="inline-block size-2.5 shrink-0 rounded-(--radius-full)"
            style={{ backgroundColor: label.color }}
          />
        ),
      }));
    }
    if (field.source === "projects") {
      return projectOptions.map((project) => ({
        id: project.id,
        label: project.name,
        searchText: project.searchText,
        icon: (
          <span
            aria-hidden="true"
            className="text-meta flex size-5 shrink-0 items-center justify-center rounded-(--radius-control) bg-(--subtle) font-medium text-(--text-muted)"
          >
            {selectProjectMark(project.logoProps, project.identifier)}
          </span>
        ),
      }));
    }
    if (field.source === "cycles") {
      return cycleOptions.map((cycle) => {
        const status = cycleStatuses.get(cycle.id);
        return {
          id: cycle.id,
          label: cycle.name,
          searchText: cycle.name,
          icon: <CycleLifecycleGlyph lifecycle={cycleLifecycle(status)} />,
        };
      });
    }
    if (field.source === "modules") {
      return moduleOptions.map((module) => ({
        id: module.id,
        label: module.name,
        icon: <Package size={14} strokeWidth={1.5} aria-hidden="true" />,
      }));
    }
    return (field.staticValues ?? []).map((option) => {
      const Icon =
        field.stem === "state_group"
          ? STATE_GROUP_ICONS[option.value as StateGroup]
          : field.stem === "priority"
            ? PRIORITY_ICONS[option.value as keyof typeof PRIORITY_ICONS]
            : undefined;
      return {
        id: option.value,
        label: option.label,
        icon: Icon === undefined ? undefined : <Icon size={14} strokeWidth={1.5} aria-hidden="true" />,
      };
    });
  }, [
    cycleOptions,
    cycleStatuses,
    field.source,
    field.staticValues,
    field.stem,
    labelOptions,
    memberOptions,
    moduleOptions,
    projectOptions,
    stateOptions,
  ]);

  // One toggle for both forms: a multi field adds and removes picks, a
  // single field replaces its value and clears when the current one is
  // re-picked (ISS-100).
  function togglePick(id: string): void {
    onChange(togglePickValue(field, condition.value, id));
  }

  const loading =
    open &&
    ((field.source === "states" && states.isPending) ||
      (field.source === "members" && (members.isPending || directory.isPending)) ||
      (field.source === "labels" && labels.isPending) ||
      (field.source === "projects" && projects.isPending) ||
      (field.source === "cycles" && cycles.isPending) ||
      (field.source === "modules" && modules.isPending));
  const failed =
    (field.source === "states" && states.isError) ||
    (field.source === "members" && (members.isError || directory.isError)) ||
    (field.source === "labels" && labels.isError) ||
    (field.source === "projects" && projects.isError) ||
    (field.source === "cycles" && cycles.isError) ||
    (field.source === "modules" && modules.isError);
  const errorText = failed ? `Couldn't load ${field.label.toLowerCase()} values.` : undefined;

  return (
    <Combobox
      trigger={
        <Button variant="ghost" size="small" data-testid="rich-value-trigger" data-empty={selected.length === 0}>
          <span className="max-w-48 truncate">{chosenLabel(selected, labelsById)}</span>
          <ChevronDown size={12} strokeWidth={2} aria-hidden="true" />
        </Button>
      }
      options={options}
      value={field.kind === "single" ? (selected[0] ?? null) : selected}
      multiple={field.kind !== "single"}
      onSelect={togglePick}
      listLabel={`${field.label} values`}
      searchLabel={`Search ${field.label.toLowerCase()} values`}
      emptyText="No matching values"
      loading={loading}
      loadingText="Loading values"
      errorText={errorText}
      open={open}
      onOpenChange={setOpen}
      visibleCap={FILTER_SECTION_CAP}
    />
  );
}

/**
 * Calendar trigger. A popover trigger is a render target: Base UI merges
 * its own click/ref props into the element, so the wrapper must forward
 * everything it receives rather than closing over a bare Button.
 */
function CalendarTrigger({
  label,
  empty,
  ...rest
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { label: string; empty: boolean }): React.ReactElement {
  return (
    <Button variant="ghost" size="small" data-testid="rich-value-trigger" data-empty={empty} {...rest}>
      <CalendarDays size={14} strokeWidth={2} aria-hidden="true" />
      <span className="max-w-48 truncate">{label}</span>
    </Button>
  );
}

/**
 * One day or one relative window; a picked day is final (no clear
 * control). Presets lead the popup and keep the custom day untouched.
 */
function DateSlot({
  field,
  value,
  onChange,
  autoOpen,
}: {
  field: RichFilterField;
  value: unknown;
  onChange: (value: unknown) => void;
  autoOpen: boolean;
}): React.ReactElement {
  const [open, setOpen] = React.useState(autoOpen);
  const weekStart = useWeekStart();
  const presets = dateValueParts(value).presets;
  const days = customDays(value);
  const label = dateValueLabel(value) ?? EMPTY_SLOT_LABEL;
  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      trigger={<CalendarTrigger label={label} empty={dateEntries(value).length === 0} />}
    >
      <PresetList value={value} onChange={onChange} />
      <DateCalendar
        value={days[0] ?? null}
        min={field.min ?? null}
        max={field.max ?? null}
        weekStart={weekStart}
        testId="rich-date-calendar"
        onSelect={(iso) => {
          onChange(composeDateValue(presets, [iso]));
          setOpen(false);
        }}
      />
    </Popover>
  );
}

/**
 * Two bounded ends. The pending half lives here so the host only ever
 * receives a complete pair; while one end is set the calendar stays open,
 * warns, and the next click fills the other end.
 */
function RangeSlot({
  field,
  value,
  onChange,
  autoOpen,
}: {
  field: RichFilterField;
  value: unknown;
  onChange: (value: unknown) => void;
  autoOpen: boolean;
}): React.ReactElement {
  const [open, setOpen] = React.useState(autoOpen);
  const weekStart = useWeekStart();
  const [pending, setPending] = React.useState<RangeValue | null>(null);
  const presets = dateValueParts(value).presets;
  const ends: RangeValue = pending ?? customRange(value);
  const incomplete = isRangeIncomplete(ends);
  // Relative windows lead the trigger when they are the only pick; a
  // custom pair reads as its ends.
  const label =
    pending === null && presets.length > 0 && ends.start === null && ends.end === null
      ? (dateValueLabel(value) ?? EMPTY_SLOT_LABEL)
      : rangeLabel(ends);
  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      trigger={
        <span data-incomplete={incomplete ? "true" : "false"} className={incomplete ? "text-(--danger)" : undefined}>
          <CalendarTrigger label={label} empty={dateEntries(value).length === 0} />
        </span>
      }
    >
      <PresetList value={value} onChange={onChange} />
      <DateRangeCalendar
        value={ends}
        min={field.min ?? null}
        max={field.max ?? null}
        weekStart={weekStart}
        testId="rich-range-calendar"
        onChange={(next) => {
          const complete = next.start !== null && next.end !== null;
          if (complete) {
            setPending(null);
            onChange(composeDateValue(presets, [next.start!, next.end!]));
            setOpen(false);
            return;
          }
          setPending(next);
          setOpen(true);
        }}
      />
    </Popover>
  );
}

/** The operator- and kind-specific editor for one condition's value. */
export function ValueSlot(props: ValueSlotProps): React.ReactElement {
  const { field, condition, onChange } = props;
  const complete = isValueCompleteForOperator(field, condition.operator, condition.value);
  const autoOpen = opensWhenIncomplete(complete);
  switch (slotEditor(field, condition.operator)) {
    case "date":
      return <DateSlot field={field} value={condition.value} onChange={onChange} autoOpen={autoOpen} />;
    case "range":
      return <RangeSlot field={field} value={condition.value} onChange={onChange} autoOpen={autoOpen} />;
    default:
      return <PickSlot {...props} autoOpen={autoOpen} />;
  }
}
