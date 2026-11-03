// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One intake date section (parity/specs/intake.md INT-010): the four
// quick presets plus the Custom row that opens the explicit-range
// dialog. The section is single-select: picking a preset replaces the
// value, re-picking it clears it, and applying a range replaces any
// preset. The dialog offers before/after/range shapes with real
// calendars; its title names the section it was opened from, and Apply
// stays disabled while a range pair is incomplete or out of order.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { DateCalendar, DateRangeCalendar, FilterSection, type RangeValue } from "../../issues/index.js";
import {
  DEFAULT_INTAKE_CUSTOM_MODE,
  INTAKE_CUSTOM_MODES,
  INTAKE_CUSTOM_RANGE_LABEL,
  INTAKE_DATE_PRESETS,
  isIntakeRangeComplete,
  sameDayNextMonth,
  type IntakeCustomMode,
  type IntakeDatePresetId,
  type IntakeDateValue,
} from "../lib/dates.js";
import { INTAKE_FILTER_SECTION_TITLES, type IntakeDateSection } from "../lib/filters.js";

export interface IntakeDateFilterProps {
  section: IntakeDateSection;
  value: IntakeDateValue | null;
  searchText: string;
  onSearchTextChange: (value: string) => void;
  today: string;
  weekStart: number;
  onPresetToggle: (preset: IntakeDatePresetId) => void;
  onCustomApply: (value: IntakeDateValue) => void;
}

/** Seed the range pair on today through the same day of next month. */
function seedRange(today: string): RangeValue {
  return { start: today, end: sameDayNextMonth(today) };
}

export function IntakeDateFilter({
  section,
  value,
  searchText,
  onSearchTextChange,
  today,
  weekStart,
  onPresetToggle,
  onCustomApply,
}: IntakeDateFilterProps): React.ReactElement {
  const title = INTAKE_FILTER_SECTION_TITLES[section];
  const [open, setOpen] = React.useState(false);
  const [mode, setMode] = React.useState<IntakeCustomMode>(DEFAULT_INTAKE_CUSTOM_MODE);
  const [range, setRange] = React.useState<RangeValue>(() => seedRange(today));
  const [day, setDay] = React.useState<string | null>(null);

  const applied = value === null ? [] : [value.kind === "preset" ? value.preset : "custom"];

  function openCustom(): void {
    if (value !== null && value.kind === "custom-range") {
      setMode("range");
      setRange({ start: value.from, end: value.to });
      setDay(null);
    } else if (value !== null && value.kind !== "preset") {
      setMode(value.kind === "custom-before" ? "before" : "after");
      setRange(seedRange(today));
      setDay(value.day);
    } else {
      setMode(DEFAULT_INTAKE_CUSTOM_MODE);
      setRange(seedRange(today));
      setDay(null);
    }
    setOpen(true);
  }

  function changeMode(next: IntakeCustomMode): void {
    if (next === mode) return;
    setMode(next);
    if (next === "range") setRange({ start: day, end: null });
    else setDay(range.start ?? range.end);
  }

  const rangeComplete = isIntakeRangeComplete(range.start, range.end);

  function apply(): void {
    // Apply is disabled while a range pair is incomplete, so a range
    // apply always carries both ends.
    if (mode === "range") {
      onCustomApply({ kind: "custom-range", from: range.start as string, to: range.end as string });
    } else if (day !== null) {
      onCustomApply(mode === "before" ? { kind: "custom-before", day } : { kind: "custom-after", day });
    }
    setOpen(false);
  }

  return (
    <>
      <FilterSection
        title={title}
        kind="single"
        groupLabel={`${title} options`}
        applied={applied}
        options={[
          ...INTAKE_DATE_PRESETS.map((preset) => ({ id: preset.id, label: preset.label })),
          { id: "custom", label: INTAKE_CUSTOM_RANGE_LABEL },
        ]}
        onToggle={(id) => {
          if (id === "custom") openCustom();
          else onPresetToggle(id as IntakeDatePresetId);
        }}
        searchText={searchText}
        onSearchTextChange={onSearchTextChange}
        hideSearch
        testId={`intake-filter-${section}`}
      />
      <Dialog
        title={`${title} — Custom`}
        open={open}
        onOpenChange={setOpen}
        actions={
          <>
            <Button variant="secondary" size="small" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <Button variant="primary" size="small" disabled={mode === "range" && !rangeComplete} onClick={apply}>
              Apply
            </Button>
          </>
        }
      >
        <div role="radiogroup" aria-label="Custom range shape" className="flex items-center gap-(--space-1)">
          {INTAKE_CUSTOM_MODES.map((option) => (
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
        {mode === "range" ? (
          <DateRangeCalendar
            value={range}
            onChange={setRange}
            weekStart={weekStart}
            testId={`intake-date-${section}-calendar`}
          />
        ) : (
          <DateCalendar
            value={day}
            onSelect={setDay}
            weekStart={weekStart}
            testId={`intake-date-${section}-calendar`}
          />
        )}
      </Dialog>
    </>
  );
}
