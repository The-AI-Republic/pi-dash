// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles list filters menu (parity/specs/cycles.md §4, CYC-006–007).
// Three sections in spec order — lifecycle states, the start window,
// the due window — on the shared filter-section chrome. Presets toggle
// in place; Custom bounds one day from either side or pairs two ends
// on the shared calendars. Picks apply immediately, the menu stays
// open while they toggle, and every change narrows the loaded rows
// without a request. The archived list offers the date windows only.

import { Button, Popover } from "@pidash/kit";
import { Check, ListFilter } from "lucide-react";
import * as React from "react";

// Deep imports: this menu is reached from an eager archives route, so the
// issues barrel would pin its whole export surface into the initial chunk.
import { DateCalendar } from "../../issues/dropdowns/DatePicker.js";
import { DateRangeCalendar } from "../../issues/dropdowns/DateRangePicker.js";
import type { RangeValue } from "../../issues/dropdowns/calendar.js";
import { useWeekStart } from "../../issues/dropdowns/weekStart.js";
import { FilterSection } from "../../issues/filters/shared/FilterSection.js";
import type { FilterSectionOption } from "../../issues/filters/shared/FilterSection.js";
import { customDateMode, customRange, isDatePreset } from "../../issues/filters/rich/dateValue.js";
import {
  DATE_PRESETS,
  customEndEntry,
  customStartEntry,
  hasCustomDate,
} from "../../issues/filters/rich/legacyDimensions.js";
import { useCycleFilterStore } from "../api/filterStore.js";
import { lifecycleLabel } from "../lib/cycleGroups.js";
import {
  countCycleFilters,
  CYCLE_FILTER_SECTION_TITLES,
  CYCLE_FILTER_STATUS_VALUES,
  type CycleDateFilterDimension,
  type CycleFilterDimension,
} from "../lib/cycleFilters.js";

/** Every dimension the live list offers, in menu order. */
export const CYCLE_FILTER_DIMENSIONS: readonly CycleFilterDimension[] = ["status", "startDate", "endDate"];

export interface CycleFilterMenuProps {
  projectId: string;
  /** Dimensions the host offers; the archived list passes the date windows only. */
  offered?: readonly CycleFilterDimension[] | undefined;
}

function CycleStatusSection({ projectId }: { projectId: string }): React.ReactElement {
  const statuses = useCycleFilterStore((state) => state.filtersFor(projectId).status);
  const options: FilterSectionOption[] = CYCLE_FILTER_STATUS_VALUES.map((value) => ({
    id: value,
    label: lifecycleLabel(value),
  }));
  return (
    <FilterSection
      title={CYCLE_FILTER_SECTION_TITLES.status}
      kind="multi"
      applied={statuses}
      options={options}
      onToggle={(value) => useCycleFilterStore.getState().toggleStatus(projectId, value)}
      testId="cycle-filter-status"
      className="py-(--space-2)"
    />
  );
}

/** The old custom dialog's three shapes; cycles never store an exact day. */
type CycleCustomMode = "after" | "before" | "range";

const CYCLE_CUSTOM_MODES: readonly { mode: CycleCustomMode; label: string }[] = [
  { mode: "after", label: "After" },
  { mode: "before", label: "Before" },
  { mode: "range", label: "Range" },
];

/** Reopen a stored custom half in its own shape, else on the range pair. */
function initialCustomMode(values: readonly string[]): CycleCustomMode {
  const stored = customDateMode(values);
  if (stored === "after" || stored === "before" || stored === "range") return stored;
  return "range";
}

/**
 * Move focus to the first day cell under `host`. Ticking Custom with
 * nothing stored lands the keyboard in the calendar it reveals; a
 * missing host or cell degrades to a no-op instead of a throw.
 */
export function focusFirstCalendarDay(host: HTMLElement | null): void {
  host?.querySelector<HTMLButtonElement>("button[data-day]")?.focus();
}

function CycleDateSection({
  projectId,
  dimension,
}: {
  projectId: string;
  dimension: CycleDateFilterDimension;
}): React.ReactElement {
  const title = CYCLE_FILTER_SECTION_TITLES[dimension];
  const stem = dimension === "startDate" ? "start-date" : "due-date";
  const values = useCycleFilterStore((state) => state.filtersFor(projectId)[dimension]);
  const presets = values.filter((entry) => isDatePreset(entry));
  const custom = hasCustomDate(values);
  const [mode, setMode] = React.useState<CycleCustomMode>(() => initialCustomMode(values));
  const [pending, setPending] = React.useState<RangeValue | null>(null);
  const weekStart = useWeekStart();
  const customRef = React.useRef<HTMLDivElement>(null);

  const ends = pending ?? customRange(values);
  const singleDay = ends.start ?? ends.end ?? null;

  const commit = (customEntries: readonly string[]): void => {
    useCycleFilterStore.getState().setDateValues(projectId, dimension, [...presets, ...customEntries]);
  };

  const toggleCustom = (): void => {
    if (custom) {
      setPending(null);
      commit([]);
      return;
    }
    focusFirstCalendarDay(customRef.current);
  };

  const changeMode = (next: CycleCustomMode): void => {
    setMode(next);
    setPending(null);
    const day = singleDay;
    if (day === null) return;
    if (next === "range") {
      setPending({ start: day, end: null });
      return;
    }
    commit([next === "before" ? customEndEntry(day) : customStartEntry(day)]);
  };

  const pickDay = (iso: string): void => {
    commit([mode === "before" ? customEndEntry(iso) : customStartEntry(iso)]);
  };

  const changeRange = (next: RangeValue): void => {
    if (next.start !== null && next.end !== null) {
      setPending(null);
      commit([customStartEntry(next.start), customEndEntry(next.end)]);
      return;
    }
    setPending(next);
  };

  const options: FilterSectionOption[] = DATE_PRESETS.map((preset) => ({ id: preset.value, label: preset.label }));
  const footer = (
    <div className="flex flex-col gap-(--space-2) p-(--space-2)">
      <button
        type="button"
        role="checkbox"
        aria-checked={custom}
        aria-label="Custom"
        onClick={toggleCustom}
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
      <div
        role="radiogroup"
        aria-label={`${title} custom mode`}
        className="flex items-center gap-(--space-1) px-(--space-3)"
      >
        {CYCLE_CUSTOM_MODES.map((option) => (
          <button
            key={option.mode}
            type="button"
            role="radio"
            aria-checked={mode === option.mode}
            aria-label={option.label}
            onClick={() => changeMode(option.mode)}
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
      <div ref={customRef}>
        {mode === "range" ? (
          <>
            {pending === null ? null : (
              <p role="status" className="text-caption px-(--space-3) pb-(--space-1) text-(--text-muted)">
                {pending.start === null ? "Pick a start date" : "Pick an end date"}
              </p>
            )}
            <DateRangeCalendar
              value={ends}
              weekStart={weekStart}
              testId={`cycle-filter-${stem}-custom-calendar`}
              onChange={changeRange}
            />
          </>
        ) : (
          <DateCalendar
            value={singleDay}
            weekStart={weekStart}
            testId={`cycle-filter-${stem}-custom-calendar`}
            onSelect={pickDay}
          />
        )}
      </div>
    </div>
  );
  return (
    <FilterSection
      title={title}
      kind="multi"
      applied={values}
      options={options}
      onToggle={(entry) => {
        const next = values.includes(entry) ? values.filter((kept) => kept !== entry) : [...values, entry];
        useCycleFilterStore.getState().setDateValues(projectId, dimension, next);
      }}
      footer={footer}
      testId={`cycle-filter-${stem}`}
      className="py-(--space-2)"
    />
  );
}

export function CycleFilterMenu({
  projectId,
  offered = CYCLE_FILTER_DIMENSIONS,
}: CycleFilterMenuProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const count = useCycleFilterStore((state) => countCycleFilters(state.filtersFor(projectId)));
  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      className="max-h-[26rem] w-80 overflow-y-auto"
      trigger={
        <Button
          variant="ghost"
          size="small"
          data-testid="cycles-filters-trigger"
          aria-label={count > 0 ? `Filters, ${count} applied` : "Filters"}
          aria-pressed={count > 0}
        >
          <ListFilter size={14} strokeWidth={2} aria-hidden="true" />
          <span>Filters</span>
          {count > 0 ? (
            <span
              data-testid="cycles-filters-count"
              aria-hidden="true"
              className="text-meta rounded-full bg-(--accent) px-(--space-2) font-medium text-(--on-accent)"
            >
              {count}
            </span>
          ) : null}
        </Button>
      }
    >
      <div data-testid="cycles-filters-panel" className="divide-y divide-(--border)">
        {offered.includes("status") ? <CycleStatusSection projectId={projectId} /> : null}
        {offered.includes("startDate") ? <CycleDateSection projectId={projectId} dimension="startDate" /> : null}
        {offered.includes("endDate") ? <CycleDateSection projectId={projectId} dimension="endDate" /> : null}
      </div>
    </Popover>
  );
}
