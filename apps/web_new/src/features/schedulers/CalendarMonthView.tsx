// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Month grid for the firing calendar (parity/specs/agents.md §5, AGT-015).
// Whole Sunday-to-Saturday weeks; a day shows up to four firings and folds
// the rest behind "+ N more". When the server capped the window, days
// switch to a "N× Name" density line instead of hundreds of blocks.

import type { SchedulerOccurrence } from "@pidash/api-client";
import * as React from "react";

import { dayKey, formatBlockTime, isSameDay, monthCells, planMonthDay, rollupDay } from "./calendarModel.js";

export interface CalendarMonthViewProps {
  anchor: Date;
  now: Date;
  byDay: ReadonlyMap<string, SchedulerOccurrence[]>;
  /** The read hit the server cap, so days render their density rollup. */
  truncated: boolean;
  onSelect: (occurrence: SchedulerOccurrence) => void;
}

const weekdayFormat = new Intl.DateTimeFormat("en-US", { weekday: "short" });
// 2024-01-07 is a Sunday.
const WEEKDAYS = Array.from({ length: 7 }, (_, index) => weekdayFormat.format(new Date(2024, 0, 7 + index)));

const PAST_BLOCK = "rgb(229, 231, 235)";

export function CalendarMonthView({
  anchor,
  now,
  byDay,
  truncated,
  onSelect,
}: CalendarMonthViewProps): React.ReactElement {
  const cells = monthCells(anchor);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-auto" data-testid="calendar-month-view">
      <div className="grid grid-cols-7 border-b border-(--border)">
        {WEEKDAYS.map((label) => (
          <div key={label} className="text-meta px-(--space-3) py-(--space-2) font-medium text-(--text-muted)">
            {label}
          </div>
        ))}
      </div>
      <div className="grid grid-cols-7">
        {cells.map((cell) => {
          const firings = byDay.get(dayKey(cell.date)) ?? [];
          const today = isSameDay(cell.date, now);
          return (
            <div
              key={cell.date.toISOString()}
              className={
                cell.inMonth
                  ? "min-h-[6rem] border-b border-l border-(--border) px-(--space-1) py-(--space-1)"
                  : "min-h-[6rem] border-b border-l border-(--border) bg-(--subtle) px-(--space-1) py-(--space-1)"
              }
            >
              <div className="mb-(--space-1) flex items-center px-(--space-1)">
                {today ? (
                  <span className="text-meta inline-flex size-5 items-center justify-center rounded-full bg-(--accent) font-medium text-(--on-accent)">
                    {cell.date.getDate()}
                  </span>
                ) : (
                  <span className="text-meta text-(--text-muted)">{cell.date.getDate()}</span>
                )}
              </div>
              <MonthDay firings={firings} truncated={truncated} onSelect={onSelect} />
            </div>
          );
        })}
      </div>
    </div>
  );
}

interface MonthDayProps {
  firings: SchedulerOccurrence[];
  truncated: boolean;
  onSelect: (occurrence: SchedulerOccurrence) => void;
}

function MonthDay({ firings, truncated, onSelect }: MonthDayProps): React.ReactElement | null {
  if (firings.length === 0) return null;
  if (truncated) {
    return (
      <div className="flex flex-col gap-px">
        {rollupDay(firings).map((rollup) => (
          <button
            key={rollup.name}
            type="button"
            className="text-meta truncate px-(--space-1) text-left text-(--text-muted)"
          >
            {rollup.label}
          </button>
        ))}
      </div>
    );
  }
  const plan = planMonthDay(firings);
  return (
    <div className="flex flex-col gap-px">
      {plan.blocks.map((occurrence) => (
        <button
          key={`${occurrence.binding_id}-${occurrence.dtstart}`}
          type="button"
          title={`${occurrence.scheduler_name} · ${formatBlockTime(new Date(occurrence.dtstart))}`}
          onClick={() => onSelect(occurrence)}
          className="text-meta flex items-center gap-(--space-1) truncate rounded-(--radius-control) px-(--space-1) py-px text-left text-(--on-accent) hover:opacity-90"
          style={{ backgroundColor: occurrence.kind === "past" ? PAST_BLOCK : occurrence.scheduler_color }}
        >
          <span className="tabular-nums opacity-90">{formatBlockTime(new Date(occurrence.dtstart))}</span>
          <span className="truncate font-medium">{occurrence.scheduler_name}</span>
        </button>
      ))}
      {plan.more === null ? null : (
        <button type="button" className="text-meta px-(--space-1) text-left text-(--text-muted)">
          {plan.more}
        </button>
      )}
    </div>
  );
}
