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

import { Avatar, Button, Popover } from "@pidash/kit";
import { CalendarDays, Check, ChevronDown, Circle, CircleCheck, CircleDashed, CircleDot, Package } from "lucide-react";
import * as React from "react";

import { useMe } from "../../../../core/session/hooks.js";
import {
  PRIORITY_ICONS,
  StateGlyph,
  STATE_GROUP_ICONS,
  selectMemberOptions,
  selectModuleOptions,
  selectProjectOptions,
  ProjectMark,
  selectStateOptions,
  usePickerCycles,
  usePickerModules,
  usePickerProjectMembers,
  usePickerProjects,
  usePickerStates,
  usePickerWorkspaceCycles,
  usePickerWorkspaceMembers,
  usePickerWorkspaceModules,
  usePickerWorkspaceStates,
  type StateGroup,
} from "../../../../shared/pickers/index.js";
import { useLabels, useWorkspaceLabels } from "../../labels/api.js";
import { DateCalendar } from "../../dropdowns/DatePicker.js";
import { DateRangeCalendar } from "../../dropdowns/DateRangePicker.js";
import { useWeekStart } from "../../dropdowns/weekStart.js";
import type { RangeValue } from "../../dropdowns/calendar.js";
import type { RichCondition, RichFilterField } from "./expression.js";
import {
  DATE_PRESETS,
  clearCustomDate,
  composeCustomDate,
  customDateMode,
  customDays,
  customRange,
  dateEntries,
  dateValueLabel,
  dateValueParts,
  hasCustomDate,
  cycleLifecycle,
  toggleDatePreset,
  type CustomDateMode,
  type CycleLifecycle,
} from "./legacyDimensions.js";
import {
  cycleFilterRows,
  orderAppliedByName,
  orderLabelOptions,
  orderMemberOptions,
  selectedIds,
} from "./filterValues.js";
import { FilterSection, type FilterSectionOption } from "../shared/FilterSection.js";
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

/**
 * The custom calendar's shape: one day, one open bound, or a pair of ends.
 * The paired form belongs to the two-end editor (the between lookup); the
 * one-day editor offers the other three.
 */
const CUSTOM_MODES: readonly { mode: CustomDateMode; label: string }[] = [
  { mode: "exact", label: "On" },
  { mode: "after", label: "After" },
  { mode: "before", label: "Before" },
  { mode: "range", label: "Range" },
];
const ONE_DAY_CUSTOM_MODES = CUSTOM_MODES.filter((option) => option.mode !== "range");

/**
 * The custom row and its shape control (ISS-092). The old custom dialog
 * asked for one day, a day after, a day before, or a pair of ends; the
 * new value list keeps that choice beside the calendar instead of behind
 * a second dialog. Ticking Custom while nothing custom is stored opens the
 * calendar for the first day; unticking it clears only the custom half and
 * leaves the relative windows.
 */
function CustomDateFooter({
  mode,
  modes,
  onModeChange,
  value,
  onChange,
  onCustom,
  children,
}: {
  mode: CustomDateMode;
  modes: readonly { mode: CustomDateMode; label: string }[];
  onModeChange: (mode: CustomDateMode) => void;
  value: unknown;
  onChange: (value: unknown) => void;
  onCustom: () => void;
  children: React.ReactNode;
}): React.ReactElement {
  const custom = hasCustomDate(value);
  return (
    <div className="flex flex-col gap-(--space-2) p-(--space-2)">
      <button
        type="button"
        role="checkbox"
        aria-checked={custom}
        aria-label="Custom"
        onClick={() => (custom ? onChange(clearCustomDate(value)) : onCustom())}
        className="flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-left hover:bg-(--subtle)"
      >
        <span
          aria-hidden="true"
          className={`grid size-3.5 shrink-0 place-items-center rounded border ${
            custom ? "border-(--accent) bg-(--accent) text-(--on-accent)" : "border-(--border)"
          }`}
        >
          {custom ? <Check size={10} strokeWidth={3} /> : null}
        </span>
        <span className="text-body truncate text-(--text)">Custom</span>
      </button>
      <div role="radiogroup" aria-label="Custom date mode" className="flex items-center gap-(--space-1) px-(--space-3)">
        {modes.map((option) => (
          <button
            key={option.mode}
            type="button"
            role="radio"
            aria-checked={mode === option.mode}
            aria-label={option.label}
            onClick={() => onModeChange(option.mode)}
            className={`text-caption rounded-(--radius-control) border px-(--space-2) py-(--space-1) ${
              mode === option.mode
                ? "border-(--accent) bg-(--accent) text-(--on-accent)"
                : "border-(--border) text-(--text-muted)"
            }`}
          >
            {option.label}
          </button>
        ))}
      </div>
      {children}
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
  // A workspace host (the workspace view dialog) carries no project, so
  // its pickers read the workspace lists instead of the project ones.
  const workspace = projectId === "";
  const states = usePickerStates(workspaceSlug, projectId, !workspace && field.source === "states" && readValues);
  const workspaceStates = usePickerWorkspaceStates(workspaceSlug, workspace && field.source === "states" && readValues);
  const members = usePickerProjectMembers(
    workspaceSlug,
    projectId,
    !workspace && field.source === "members" && readValues
  );
  const directory = usePickerWorkspaceMembers(workspaceSlug, field.source === "members" && readValues);
  const labels = useLabels(workspaceSlug, projectId, !workspace && field.source === "labels" && readValues);
  const workspaceLabels = useWorkspaceLabels(workspaceSlug, workspace && field.source === "labels" && readValues);
  // Project, cycle and module picks borrow the shared pickers' thin reads;
  // all three stay lazy until opened or while resolving stored ids.
  const projects = usePickerProjects(workspaceSlug, field.source === "projects" && readValues);
  const cycles = usePickerCycles(workspaceSlug, projectId, !workspace && field.source === "cycles" && readValues);
  const workspaceCycles = usePickerWorkspaceCycles(workspaceSlug, workspace && field.source === "cycles" && readValues);
  const modules = usePickerModules(workspaceSlug, projectId, !workspace && field.source === "modules" && readValues);
  const workspaceModules = usePickerWorkspaceModules(
    workspaceSlug,
    workspace && field.source === "modules" && readValues
  );
  const me = useMe();
  const meId = me.data?.id ?? null;

  const stateRows = workspace ? (workspaceStates.data ?? []) : (states.data ?? []);
  const labelRows = workspace ? (workspaceLabels.data ?? []) : (labels.data ?? []);
  // Both cycle projections carry identity and name; only the project
  // rows annotate a lifecycle marker for the glyph.
  const cycleRows: readonly { id: string; name: string; status?: string | undefined }[] = workspace
    ? (workspaceCycles.data ?? [])
    : (cycles.data ?? []);
  const moduleRows = workspace ? (workspaceModules.data ?? []) : (modules.data ?? []);

  // Applied states lead and the rest keep server order (ISS-082).
  const stateOptions = React.useMemo(() => {
    if (field.source !== "states") return [];
    const chosenSet = new Set(selected);
    const options = selectStateOptions(stateRows);
    return [
      ...options.filter((option) => chosenSet.has(option.id)),
      ...options.filter((option) => !chosenSet.has(option.id)),
    ];
  }, [field.source, selected, stateRows]);

  const memberOptions = React.useMemo(() => {
    if (field.source !== "members" || directory.data === undefined) return [];
    // Without a project the directory itself is the option source.
    if (!workspace && members.data === undefined) return [];
    return orderMemberOptions(
      selectMemberOptions({
        projectMembers: workspace ? undefined : members.data,
        directory: directory.data,
        meId,
        selectedIds: selected,
      }),
      selected,
      meId
    );
  }, [directory.data, field.source, meId, members.data, selected, workspace]);

  const labelOptions = React.useMemo(
    () => (field.source === "labels" ? orderLabelOptions(labelRows, selected) : []),
    [field.source, labelRows, selected]
  );

  // Applied picks lead, then names alphabetically (ISS-089–091). The cycle
  // list keeps every project cycle, completed ones included, so each
  // lifecycle glyph is reachable; it has no No-cycle clear row, which has
  // no id to filter on.
  const projectOptions = React.useMemo(
    () =>
      field.source === "projects"
        ? orderAppliedByName(selectProjectOptions({ projects: projects.data ?? [], selectedIds: [] }), selected)
        : [],
    [field.source, projects.data, selected]
  );
  const cycleOptions = React.useMemo(
    () => (field.source === "cycles" ? orderAppliedByName(cycleFilterRows(cycleRows), selected) : []),
    [cycleRows, field.source, selected]
  );
  const moduleOptions = React.useMemo(
    () =>
      field.source === "modules"
        ? orderAppliedByName(selectModuleOptions({ modules: moduleRows, selectedIds: [], single: false }), selected)
        : [],
    [field.source, moduleRows, selected]
  );
  // Cycle rows keep the status the option list dropped; the running one
  // pulses and each lifecycle gets its own glyph.
  const cycleStatuses = React.useMemo(() => {
    const map = new Map<string, string | undefined>();
    for (const cycle of cycleRows) map.set(cycle.id, cycle.status);
    return map;
  }, [cycleRows]);

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

  const options: FilterSectionOption[] = React.useMemo(() => {
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
        icon: <ProjectMark logoProps={project.logoProps} identifier={project.identifier} size={20} />,
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

  // Pending and error flags follow the active read: the project hooks
  // stay idle in workspace scope, so their flags would mislead.
  const statesLoading = workspace ? workspaceStates.isPending : states.isPending;
  const membersLoading = workspace ? directory.isPending : members.isPending || directory.isPending;
  const labelsLoading = workspace ? workspaceLabels.isPending : labels.isPending;
  const cyclesLoading = workspace ? workspaceCycles.isPending : cycles.isPending;
  const modulesLoading = workspace ? workspaceModules.isPending : modules.isPending;
  const loading =
    open &&
    ((field.source === "states" && statesLoading) ||
      (field.source === "members" && membersLoading) ||
      (field.source === "labels" && labelsLoading) ||
      (field.source === "projects" && projects.isPending) ||
      (field.source === "cycles" && cyclesLoading) ||
      (field.source === "modules" && modulesLoading));
  const statesFailed = workspace ? workspaceStates.isError : states.isError;
  const membersFailed = workspace ? directory.isError : members.isError || directory.isError;
  const labelsFailed = workspace ? workspaceLabels.isError : labels.isError;
  const cyclesFailed = workspace ? workspaceCycles.isError : cycles.isError;
  const modulesFailed = workspace ? workspaceModules.isError : modules.isError;
  const failed =
    (field.source === "states" && statesFailed) ||
    (field.source === "members" && membersFailed) ||
    (field.source === "labels" && labelsFailed) ||
    (field.source === "projects" && projects.isError) ||
    (field.source === "cycles" && cyclesFailed) ||
    (field.source === "modules" && modulesFailed);
  const errorText = failed ? `Couldn't load ${field.label.toLowerCase()} values.` : undefined;

  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      trigger={
        <Button variant="ghost" size="small" data-testid="rich-value-trigger" data-empty={selected.length === 0}>
          <span className="max-w-48 truncate">{chosenLabel(selected, labelsById)}</span>
          <ChevronDown size={12} strokeWidth={2} aria-hidden="true" />
        </Button>
      }
      className="max-h-[24rem] w-80 overflow-y-auto"
    >
      <FilterSection
        title={field.label}
        kind={field.kind === "single" ? "single" : "multi"}
        applied={selected}
        options={options}
        onToggle={(id) => {
          togglePick(id);
          // A single-choice slot is complete after one pick, so it closes;
          // a multi slot stays open for the next value.
          if (field.kind === "single") setOpen(false);
        }}
        loading={loading}
        errorText={errorText}
        testId={`rich-section-${field.stem}`}
        groupLabel={`${field.label} values`}
      />
    </Popover>
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
 * One day or one relative window (ISS-092). The list opens with the four
 * relative windows in the shared section chrome and, below them, the
 * custom row and calendar: On stores the picked day exactly, After stores
 * it as the open lower bound, and Before as the open upper bound; the
 * paired form lives under the between lookup. Relative windows and the
 * custom half combine, and each toggle leaves the other half as it was.
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
  const [mode, setMode] = React.useState<CustomDateMode>(() => {
    const stored = customDateMode(value);
    // The paired form belongs to the between lookup; a stored pair read
    // here still opens on its first day under the one-day modes.
    return stored === "range" ? "exact" : (stored ?? "exact");
  });
  const customRef = React.useRef<HTMLDivElement>(null);
  const weekStart = useWeekStart();
  const presets = dateValueParts(value).presets;
  const days = customDays(value);
  const label = dateValueLabel(value) ?? EMPTY_SLOT_LABEL;
  const focusCalendar = (): void => {
    setOpen(true);
    // The calendar sits below the rows; bring the first day into view and
    // focus it once the popup has mounted.
    window.requestAnimationFrame(() => {
      customRef.current?.querySelector<HTMLButtonElement>("[data-day]")?.focus();
    });
  };
  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      trigger={<CalendarTrigger label={label} empty={dateEntries(value).length === 0} />}
    >
      <FilterSection
        title={field.label}
        kind="multi"
        applied={[...presets, ...(hasCustomDate(value) ? ["custom"] : [])]}
        options={DATE_PRESETS.map((preset) => ({ id: preset.value, label: preset.label }))}
        onToggle={(id) => onChange(toggleDatePreset(value, id))}
        testId={`rich-section-${field.stem}`}
        className="w-64"
        footer={
          <CustomDateFooter
            mode={mode}
            modes={ONE_DAY_CUSTOM_MODES}
            onModeChange={(next) => {
              setMode(next);
              const day = days[0] ?? null;
              if (day !== null) onChange(composeCustomDate(presets, next, day, null));
              focusCalendar();
            }}
            value={value}
            onChange={onChange}
            onCustom={focusCalendar}
          >
            <div ref={customRef}>
              <DateCalendar
                value={days[0] ?? null}
                min={field.min ?? null}
                max={field.max ?? null}
                weekStart={weekStart}
                testId="rich-date-calendar"
                onSelect={(iso) => {
                  onChange(composeCustomDate(presets, mode, iso, null));
                  setOpen(false);
                }}
              />
            </div>
          </CustomDateFooter>
        }
      />
    </Popover>
  );
}

/**
 * Two bounded ends under the between lookup (ISS-092). The same shared
 * section leads with the relative windows; below them the custom row and
 * a two-end calendar commit a pair, while After and Before store one open
 * bound the same way the one-day calendar does. The pending first end
 * lives here so the host only ever receives a complete pair; while one end
 * is set the calendar stays open, warns, and the next click fills the
 * other end.
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
  const [mode, setMode] = React.useState<CustomDateMode>(() => {
    const stored = customDateMode(value);
    return stored === "exact" ? "range" : (stored ?? "range");
  });
  const customRef = React.useRef<HTMLDivElement>(null);
  const weekStart = useWeekStart();
  const [pending, setPending] = React.useState<RangeValue | null>(null);
  const presets = dateValueParts(value).presets;
  const ends: RangeValue = pending ?? customRange(value);
  const incomplete = isRangeIncomplete(ends);
  // Relative windows lead the trigger when they are the only pick; a
  // custom pair reads as its ends. A preset-only value always has a label.
  const label =
    pending === null && presets.length > 0 && ends.start === null && ends.end === null
      ? (dateValueLabel(value) as string)
      : rangeLabel(ends);
  const focusCalendar = (): void => {
    setOpen(true);
    window.requestAnimationFrame(() => {
      customRef.current?.querySelector<HTMLButtonElement>("[data-day]")?.focus();
    });
  };
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
      <FilterSection
        title={field.label}
        kind="multi"
        applied={[...presets, ...(hasCustomDate(value) ? ["custom"] : [])]}
        options={DATE_PRESETS.map((preset) => ({ id: preset.value, label: preset.label }))}
        onToggle={(id) => onChange(toggleDatePreset(value, id))}
        testId={`rich-section-${field.stem}`}
        className="w-64"
        footer={
          <CustomDateFooter
            mode={mode}
            modes={CUSTOM_MODES}
            onModeChange={(next) => {
              setMode(next);
              setPending(null);
              const day = ends.start ?? ends.end;
              if (day !== null) onChange(composeCustomDate(presets, next, day, null));
              focusCalendar();
            }}
            value={value}
            onChange={onChange}
            onCustom={focusCalendar}
          >
            <div ref={customRef}>
              {mode === "range" ? (
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
                      onChange(composeCustomDate(presets, "range", next.start, next.end));
                      setOpen(false);
                      return;
                    }
                    setPending(next);
                    setOpen(true);
                  }}
                />
              ) : (
                <DateCalendar
                  value={ends.start ?? ends.end ?? null}
                  min={field.min ?? null}
                  max={field.max ?? null}
                  weekStart={weekStart}
                  testId="rich-date-calendar"
                  onSelect={(iso) => {
                    onChange(composeCustomDate(presets, mode, iso, null));
                    setOpen(false);
                  }}
                />
              )}
            </div>
          </CustomDateFooter>
        }
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
