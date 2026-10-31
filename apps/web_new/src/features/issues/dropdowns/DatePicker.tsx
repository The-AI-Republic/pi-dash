// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Single-date picker (parity/specs/issues.md §17, ISS-214). A trigger
// showing the picked day or a placeholder, a hover clear resetting to
// null, and a portalled month calendar with month/year caption selects,
// week-start-aware headers, and min/max disabling days both directions
// (start stays at or before due). The bare DateCalendar is exported for
// surfaces that compose their own trigger.

import { IconButton, Popover, cn } from "@pidash/kit";
import { CalendarDays, ChevronLeft, ChevronRight, X } from "lucide-react";
import * as React from "react";

import {
  CAPTION_YEAR_MAX,
  CAPTION_YEAR_MIN,
  DEFAULT_WEEK_START,
  formatDay,
  formatDayLong,
  MONTH_NAMES,
  monthGrid,
  normalizeWeekStart,
  orderedWeekdays,
  parseIsoDay,
  shiftMonth,
  todayIsoDay,
} from "./calendar.js";

export interface DateCalendarProps {
  /** Selected ISO day, or null for none. */
  value: string | null;
  onSelect: (iso: string) => void;
  min?: string | null | undefined;
  max?: string | null | undefined;
  weekStart?: number | undefined;
  testId?: string | undefined;
}

function initialVisibleMonth(value: string | null): { year: number; month0: number } {
  const parsed = parseIsoDay(value);
  if (parsed !== null) {
    return { year: Math.min(CAPTION_YEAR_MAX, Math.max(CAPTION_YEAR_MIN, parsed.year)), month0: parsed.month0 };
  }
  const today = parseIsoDay(todayIsoDay()) as { year: number; month0: number; day: number };
  return { year: today.year, month0: today.month0 };
}

/** Month calendar: caption selects plus the day grid. No popover. */
export function DateCalendar({
  value,
  onSelect,
  min,
  max,
  weekStart = DEFAULT_WEEK_START,
  testId = "date-calendar",
}: DateCalendarProps): React.ReactElement {
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
          const selected = cell.iso === value;
          return (
            <button
              key={cell.iso}
              type="button"
              data-day={cell.iso}
              data-in-month={cell.inMonth ? "true" : "false"}
              data-today={cell.isToday ? "true" : undefined}
              aria-label={formatDayLong(cell.iso)}
              aria-pressed={selected}
              disabled={cell.disabled}
              onClick={() => onSelect(cell.iso)}
              className={
                selected
                  ? "text-body flex h-8 items-center justify-center rounded-(--radius-control) bg-(--accent) font-medium text-(--bg)"
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

export interface DatePickerProps {
  /** Selected ISO day, or null for none. */
  value: string | null;
  onChange: (iso: string | null) => void;
  min?: string | null | undefined;
  max?: string | null | undefined;
  /** Sunday-first week start; consumers read it from the profile. */
  weekStart?: number | undefined;
  /** Accessible name shared by the trigger and the clear control. */
  label: string;
  placeholder?: string | undefined;
  disabled?: boolean | undefined;
  /** Hover clear resetting to null. Defaults to true. */
  clearable?: boolean | undefined;
  /** Danger day styling for an overdue value (ISS-153); the trigger carries it too. */
  overdue?: boolean | undefined;
  testId?: string | undefined;
  /** Explicit keyboard position inside a dialog's tab order. */
  tabIndex?: number | undefined;
  /** Fully custom trigger; overrides the default value button. */
  trigger?: React.ReactElement<Record<string, unknown>> | undefined;
  /** Controlled popup state; omit for the picker's own state. */
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
}

/** Single-date dropdown: trigger plus the portalled month calendar. */
export function DatePicker({
  value,
  onChange,
  min,
  max,
  weekStart = DEFAULT_WEEK_START,
  label,
  placeholder = "Select date",
  disabled = false,
  clearable = true,
  overdue = false,
  testId = "date-picker",
  tabIndex,
  trigger,
  open: controlledOpen,
  onOpenChange,
}: DatePickerProps): React.ReactElement {
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(false);
  const open = controlledOpen ?? uncontrolledOpen;
  function setOpen(next: boolean): void {
    if (controlledOpen === undefined) setUncontrolledOpen(next);
    onOpenChange?.(next);
  }
  const normalizedStart = normalizeWeekStart(weekStart);
  const showClear = clearable && value !== null && !disabled;

  return (
    <div
      data-testid={testId}
      {...(tabIndex === undefined ? {} : { tabIndex })}
      className="group inline-flex items-center gap-(--space-1)"
    >
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
              className={cn(
                "text-body inline-flex items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) hover:bg-(--subtle) disabled:cursor-not-allowed disabled:hover:bg-transparent",
                value === null ? "text-(--text-muted)" : overdue ? "text-(--danger)" : "text-(--text)"
              )}
            >
              <CalendarDays size={14} strokeWidth={2} aria-hidden="true" className="shrink-0" />
              {value === null ? placeholder : formatDay(value)}
            </button>
          )
        }
      >
        <DateCalendar
          value={value}
          min={min}
          max={max}
          weekStart={normalizedStart}
          testId={`${testId}-calendar`}
          onSelect={(iso) => {
            onChange(iso);
            setOpen(false);
          }}
        />
      </Popover>
      {showClear ? (
        <button
          type="button"
          aria-label={`Clear ${label}`}
          data-testid={`${testId}-clear`}
          onClick={() => onChange(null)}
          className="rounded-(--radius-control) p-(--space-1) text-(--text-muted) opacity-0 group-hover:opacity-100 hover:bg-(--subtle) hover:text-(--text) focus-visible:opacity-100"
        >
          <X size={14} strokeWidth={2} aria-hidden="true" />
        </button>
      ) : null}
    </div>
  );
}
