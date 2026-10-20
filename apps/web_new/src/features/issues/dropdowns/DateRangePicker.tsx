// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Date-range picker (parity/specs/issues.md §17, ISS-215). A merged
// trigger with one smart label or a split trigger with two spans joined
// by an arrow, opening a portalled range calendar where each click
// adjusts exactly one end (a same-day double pick resets). Clearing
// resets both ends through a single update — never two concurrent
// single-field writes (bug NEWFRONT-150). The bare DateRangeCalendar is
// exported for surfaces that compose their own trigger.

import { IconButton, Popover } from "@pidash/kit";
import { ArrowRight, CalendarDays, ChevronLeft, ChevronRight, X } from "lucide-react";
import * as React from "react";

import {
  applyRangeClick,
  CAPTION_YEAR_MAX,
  CAPTION_YEAR_MIN,
  DEFAULT_WEEK_START,
  formatDay,
  formatDayLong,
  formatMergedRange,
  isDayInRange,
  MONTH_NAMES,
  monthGrid,
  normalizeWeekStart,
  orderedWeekdays,
  parseIsoDay,
  shiftMonth,
  todayIsoDay,
  type RangeValue,
} from "./calendar.js";

/** Trigger shape: one smart label, or two spans joined by an arrow. */
export type DateRangeMode = "merged" | "split";

export interface DateRangeCalendarProps {
  value: RangeValue;
  onChange: (range: RangeValue) => void;
  min?: string | null | undefined;
  max?: string | null | undefined;
  weekStart?: number | undefined;
  testId?: string | undefined;
}

function initialVisibleMonth(value: RangeValue): { year: number; month0: number } {
  const anchor = parseIsoDay(value.start) ?? parseIsoDay(value.end);
  if (anchor !== null) {
    return { year: Math.min(CAPTION_YEAR_MAX, Math.max(CAPTION_YEAR_MIN, anchor.year)), month0: anchor.month0 };
  }
  const today = parseIsoDay(todayIsoDay()) as { year: number; month0: number; day: number };
  return { year: today.year, month0: today.month0 };
}

/** Range calendar: caption selects plus the day grid. No popover. */
export function DateRangeCalendar({
  value,
  onChange,
  min,
  max,
  weekStart = DEFAULT_WEEK_START,
  testId = "date-range-calendar",
}: DateRangeCalendarProps): React.ReactElement {
  const [visible, setVisible] = React.useState(() => initialVisibleMonth(value));
  const weeks = monthGrid(visible.year, visible.month0, weekStart, min ?? null, max ?? null);
  const headers = orderedWeekdays(weekStart);
  const years = React.useMemo(
    () => Array.from({ length: CAPTION_YEAR_MAX - CAPTION_YEAR_MIN + 1 }, (_, index) => CAPTION_YEAR_MIN + index),
    []
  );

  return (
    <div data-testid={testId} className="flex flex-col gap-(--space-4)">
      <div className="flex items-center gap-(--space-2)">
        <IconButton
          label="Previous month"
          size="small"
          onClick={() => setVisible(shiftMonth(visible.year, visible.month0, -1))}
        >
          <ChevronLeft size={14} strokeWidth={2} aria-hidden="true" />
        </IconButton>
        <select
          aria-label="Month"
          value={visible.month0}
          onChange={(event) => setVisible({ year: visible.year, month0: Number(event.target.value) })}
          className="text-body min-w-0 flex-1 rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) py-(--space-1) text-(--text)"
        >
          {MONTH_NAMES.map((name, index) => (
            <option key={name} value={index}>
              {name}
            </option>
          ))}
        </select>
        <select
          aria-label="Year"
          value={visible.year}
          onChange={(event) => setVisible({ year: Number(event.target.value), month0: visible.month0 })}
          className="text-body rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) py-(--space-1) text-(--text)"
        >
          {years.map((year) => (
            <option key={year} value={year}>
              {year}
            </option>
          ))}
        </select>
        <IconButton
          label="Next month"
          size="small"
          onClick={() => setVisible(shiftMonth(visible.year, visible.month0, 1))}
        >
          <ChevronRight size={14} strokeWidth={2} aria-hidden="true" />
        </IconButton>
      </div>
      <div className="grid grid-cols-7 gap-(--space-1)">
        {headers.map((day) => (
          <abbr
            key={day.full}
            title={day.full}
            data-testid={`${testId}-weekday-${day.short}`}
            className="text-meta flex h-(--space-6) items-center justify-center text-(--text-muted) no-underline"
          >
            {day.short}
          </abbr>
        ))}
      </div>
      <div className="grid grid-cols-7 gap-(--space-1)" data-testid={`${testId}-grid`}>
        {weeks.flat().map((cell) => {
          const endpoint = cell.iso === value.start || cell.iso === value.end;
          const interior = isDayInRange(cell.iso, value);
          return (
            <button
              key={cell.iso}
              type="button"
              data-day={cell.iso}
              data-in-month={cell.inMonth ? "true" : "false"}
              data-in-range={interior ? "true" : undefined}
              data-today={cell.isToday ? "true" : undefined}
              aria-label={formatDayLong(cell.iso)}
              aria-pressed={endpoint}
              disabled={cell.disabled}
              onClick={() => onChange(applyRangeClick(value, cell.iso))}
              className={
                endpoint
                  ? "text-body flex h-8 items-center justify-center rounded-(--radius-control) bg-(--accent) font-medium text-(--bg)"
                  : interior
                    ? "text-body flex h-8 items-center justify-center rounded-(--radius-control) bg-(--accent-subtle) text-(--text) disabled:cursor-not-allowed disabled:opacity-40"
                    : cell.inMonth
                      ? "text-body flex h-8 items-center justify-center rounded-(--radius-control) text-(--text) hover:bg-(--subtle) disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent"
                      : "text-body flex h-8 items-center justify-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent"
              }
            >
              {cell.day}
            </button>
          );
        })}
      </div>
    </div>
  );
}

export interface DateRangePickerProps {
  value: RangeValue;
  onChange: (range: RangeValue) => void;
  mode: DateRangeMode;
  min?: string | null | undefined;
  max?: string | null | undefined;
  /** Sunday-first week start; consumers read it from the profile. */
  weekStart?: number | undefined;
  /** Accessible name shared by the trigger and the clear control. */
  label: string;
  mergedPlaceholder?: string | undefined;
  fromPlaceholder?: string | undefined;
  toPlaceholder?: string | undefined;
  disabled?: boolean | undefined;
  /** Hover clear resetting both ends. Defaults to false. */
  clearable?: boolean | undefined;
  testId?: string | undefined;
  /** Fully custom trigger; overrides the default value button. */
  trigger?: React.ReactElement<Record<string, unknown>> | undefined;
  /** Controlled popup state; omit for the picker's own state. */
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
}

function mergedLabel(
  value: RangeValue,
  mergedPlaceholder: string,
  fromPlaceholder: string,
  toPlaceholder: string
): string {
  if (value.start !== null && value.end !== null) return formatMergedRange(value.start, value.end);
  if (value.start !== null) return `${formatDay(value.start)} - ${toPlaceholder}`;
  if (value.end !== null) return `${fromPlaceholder} - ${formatDay(value.end)}`;
  return mergedPlaceholder;
}

/** Date-range dropdown: merged or split trigger plus the portalled range calendar. */
export function DateRangePicker({
  value,
  onChange,
  mode,
  min,
  max,
  weekStart = DEFAULT_WEEK_START,
  label,
  mergedPlaceholder = "Select dates",
  fromPlaceholder = "Start date",
  toPlaceholder = "End date",
  disabled = false,
  clearable = false,
  testId = "date-range-picker",
  trigger,
  open: controlledOpen,
  onOpenChange,
}: DateRangePickerProps): React.ReactElement {
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(false);
  const open = controlledOpen ?? uncontrolledOpen;
  function setOpen(next: boolean): void {
    if (controlledOpen === undefined) setUncontrolledOpen(next);
    onOpenChange?.(next);
  }
  const normalizedStart = normalizeWeekStart(weekStart);
  const showClear = clearable && (value.start !== null || value.end !== null) && !disabled;
  const empty = value.start === null && value.end === null;

  return (
    <div data-testid={testId} className="group inline-flex items-center gap-(--space-1)">
      <Popover
        open={open}
        onOpenChange={setOpen}
        trigger={
          trigger ?? (
            <button
              type="button"
              aria-label={label}
              disabled={disabled}
              data-testid={`${testId}-trigger`}
              className={
                empty
                  ? "text-body inline-flex items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) hover:bg-(--subtle) disabled:cursor-not-allowed disabled:hover:bg-transparent"
                  : "text-body inline-flex items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text) hover:bg-(--subtle) disabled:cursor-not-allowed disabled:hover:bg-transparent"
              }
            >
              <CalendarDays size={14} strokeWidth={2} aria-hidden="true" className="shrink-0" />
              {mode === "merged" ? (
                mergedLabel(value, mergedPlaceholder, fromPlaceholder, toPlaceholder)
              ) : (
                <span className="inline-flex items-center gap-(--space-2)">
                  <span className={value.start === null ? "text-(--text-muted)" : undefined}>
                    {value.start === null ? fromPlaceholder : formatDay(value.start)}
                  </span>
                  <ArrowRight size={14} strokeWidth={2} aria-hidden="true" className="shrink-0" />
                  <span className={value.end === null ? "text-(--text-muted)" : undefined}>
                    {value.end === null ? toPlaceholder : formatDay(value.end)}
                  </span>
                </span>
              )}
            </button>
          )
        }
      >
        <DateRangeCalendar
          value={value}
          min={min}
          max={max}
          weekStart={normalizedStart}
          testId={`${testId}-calendar`}
          onChange={onChange}
        />
      </Popover>
      {showClear ? (
        <button
          type="button"
          aria-label={`Clear ${label}`}
          data-testid={`${testId}-clear`}
          onClick={() => onChange({ start: null, end: null })}
          className="rounded-(--radius-control) p-(--space-1) text-(--text-muted) opacity-0 group-hover:opacity-100 hover:bg-(--subtle) hover:text-(--text) focus-visible:opacity-100"
        >
          <X size={14} strokeWidth={2} aria-hidden="true" />
        </button>
      ) : null}
    </div>
  );
}
