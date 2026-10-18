// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Calendar header (parity/specs/issues.md §4, ISS-022/023). Previous and
// next step one month or one week; Today returns to the current date;
// the title opens a month picker with year stepping (disabled in week
// view). The Options popover switches mode and weekend columns and
// closes itself after a pick on narrow screens.

import { Button, IconButton, Popover } from "@pidash/kit";
import { Check, ChevronLeft, ChevronRight, SlidersHorizontal } from "lucide-react";
import * as React from "react";

import { shortMonthTitles } from "./model.js";
import { useNarrowViewport } from "./useMedia.js";

export interface CalendarOptionsValue {
  mode: "month" | "week";
  showWeekends: boolean;
}

export interface CalendarHeaderProps {
  title: string;
  mode: "month" | "week";
  showWeekends: boolean;
  /** Visible month for the picker; ignored in week mode. */
  pickerYear: number;
  pickerMonth0: number;
  onPrev: () => void;
  onNext: () => void;
  onToday: () => void;
  onPickMonth: (month0: number, year: number) => void;
  onOptionsChange: (next: Partial<CalendarOptionsValue>) => void;
}

export function CalendarHeader({
  title,
  mode,
  showWeekends,
  pickerYear,
  pickerMonth0,
  onPrev,
  onNext,
  onToday,
  onPickMonth,
  onOptionsChange,
}: CalendarHeaderProps): React.ReactElement {
  const narrow = useNarrowViewport();
  const [pickerOpen, setPickerOpen] = React.useState(false);
  const [pickerYearState, setPickerYearState] = React.useState(pickerYear);
  const [optionsOpen, setOptionsOpen] = React.useState(false);

  React.useEffect(() => {
    setPickerYearState(pickerYear);
  }, [pickerYear]);

  const months = shortMonthTitles();

  return (
    <div className="mb-(--space-4) flex items-center justify-between gap-(--space-2)">
      <div className="relative flex items-center gap-1.5">
        <IconButton label="Previous" size="small" onClick={onPrev}>
          <ChevronLeft size={16} strokeWidth={2} aria-hidden="true" />
        </IconButton>
        <IconButton label="Next" size="small" onClick={onNext}>
          <ChevronRight size={16} strokeWidth={2} aria-hidden="true" />
        </IconButton>
        <button
          type="button"
          className="text-18 text-[18px] font-semibold text-(--text)"
          disabled={mode === "week"}
          aria-haspopup="dialog"
          aria-expanded={pickerOpen}
          onClick={() => setPickerOpen((open) => !open)}
        >
          {title}
        </button>
        {pickerOpen && mode === "month" ? (
          <div className="shadow-popover absolute top-full left-0 z-30 mt-(--space-2) w-56 rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-4)">
            <div className="mb-(--space-3) flex items-center justify-between gap-(--space-2)">
              <IconButton label="Previous year" size="small" onClick={() => setPickerYearState((year) => year - 1)}>
                <ChevronLeft size={14} strokeWidth={2} aria-hidden="true" />
              </IconButton>
              <span className="text-11 font-medium text-(--text)">{pickerYearState}</span>
              <IconButton label="Next year" size="small" onClick={() => setPickerYearState((year) => year + 1)}>
                <ChevronRight size={14} strokeWidth={2} aria-hidden="true" />
              </IconButton>
            </div>
            <div className="grid grid-cols-4 gap-(--space-1)">
              {months.map((month, index) => (
                <button
                  key={month}
                  type="button"
                  aria-pressed={index === pickerMonth0 && pickerYearState === pickerYear}
                  onClick={() => {
                    onPickMonth(index, pickerYearState);
                    setPickerOpen(false);
                  }}
                  className={
                    index === pickerMonth0 && pickerYearState === pickerYear
                      ? "text-meta rounded-(--radius-control) bg-(--accent) px-(--space-2) py-(--space-1) font-medium text-(--on-accent)"
                      : "text-meta rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text) hover:bg-(--subtle)"
                  }
                >
                  {month}
                </button>
              ))}
            </div>
          </div>
        ) : null}
      </div>
      <div className="flex items-center gap-1.5">
        <Button variant="ghost" size="small" onClick={onToday}>
          Today
        </Button>
        {narrow ? (
          <Popover
            open={optionsOpen}
            onOpenChange={setOptionsOpen}
            trigger={
              <span className="inline-flex">
                <IconButton label="Calendar options" size="small">
                  <SlidersHorizontal size={14} strokeWidth={2} aria-hidden="true" />
                </IconButton>
              </span>
            }
            className="min-w-[12rem]"
          >
            <OptionsPanel
              mode={mode}
              showWeekends={showWeekends}
              onChange={(next) => {
                onOptionsChange(next);
                setOptionsOpen(false);
              }}
            />
          </Popover>
        ) : (
          <Popover
            open={optionsOpen}
            onOpenChange={setOptionsOpen}
            trigger={
              <Button variant="secondary" size="small">
                Options
              </Button>
            }
            className="min-w-[12rem]"
          >
            <OptionsPanel mode={mode} showWeekends={showWeekends} onChange={onOptionsChange} />
          </Popover>
        )}
      </div>
    </div>
  );
}

function OptionsPanel({
  mode,
  showWeekends,
  onChange,
}: {
  mode: "month" | "week";
  showWeekends: boolean;
  onChange: (next: Partial<CalendarOptionsValue>) => void;
}): React.ReactElement {
  return (
    <div className="flex flex-col gap-(--space-1)">
      <OptionRow label="Month layout" active={mode === "month"} onSelect={() => onChange({ mode: "month" })} />
      <OptionRow label="Week layout" active={mode === "week"} onSelect={() => onChange({ mode: "week" })} />
      <OptionRow
        label="Show weekends"
        active={showWeekends}
        onSelect={() => onChange({ showWeekends: !showWeekends })}
      />
    </div>
  );
}

function OptionRow({
  label,
  active,
  onSelect,
}: {
  label: string;
  active: boolean;
  onSelect: () => void;
}): React.ReactElement {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onSelect}
      className="text-body flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left text-(--text) hover:bg-(--subtle)"
    >
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {active ? <Check size={14} strokeWidth={2} aria-hidden="true" className="text-(--text-muted)" /> : null}
    </button>
  );
}
