// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Week grid for the firing calendar (parity/specs/agents.md §5, AGT-015).
// Seven Sunday-to-Saturday lanes of 24 hours; each firing sits at its
// minute offset in its day, and a line marks the current time on today's
// lane. Firings that start in the same minute split their lane side by
// side so each stays visible and clickable. Blocks are buttons with a
// title: click to inspect, nothing else.

import type { SchedulerOccurrence } from "@pidash/api-client";
import * as React from "react";

import {
  DAY_HEIGHT,
  HOUR_HEIGHT,
  assignWeekLanes,
  dayKey,
  formatBlockTime,
  formatDayLabel,
  isSameDay,
  occurrenceTop,
  timeLineTop,
  weekDays,
} from "./calendarModel.js";

export interface CalendarWeekViewProps {
  anchor: Date;
  now: Date;
  /** Firings grouped by local day key. */
  byDay: ReadonlyMap<string, SchedulerOccurrence[]>;
  onSelect: (occurrence: SchedulerOccurrence) => void;
}

const HOURS = Array.from({ length: 24 }, (_, hour) => hour);

const PAST_BLOCK = "rgb(229, 231, 235)";

export function CalendarWeekView({ anchor, now, byDay, onSelect }: CalendarWeekViewProps): React.ReactElement {
  const days = weekDays(anchor);
  const lineTop = timeLineTop(now);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-auto" data-testid="calendar-week-view">
      <div className="grid grid-cols-[3.5rem_repeat(7,minmax(0,1fr))] border-b border-(--border)">
        <div aria-hidden="true" />
        {days.map((day) => (
          <div
            key={day.toISOString()}
            data-testid="calendar-week-day-head"
            className="flex items-center gap-(--space-2) px-(--space-3) py-(--space-2)"
          >
            <span className="text-meta font-medium text-(--text-muted)">{formatDayLabel(day)}</span>
            {isSameDay(day, now) ? (
              <span
                data-testid="calendar-week-today"
                className="text-meta rounded-(--radius-control) bg-(--accent-subtle) px-(--space-2) font-medium text-(--accent)"
              >
                Today
              </span>
            ) : null}
          </div>
        ))}
      </div>
      <div className="grid grid-cols-[3.5rem_repeat(7,minmax(0,1fr))]">
        <div className="flex flex-col">
          {HOURS.map((hour) => (
            <div
              key={hour}
              style={{ height: `${HOUR_HEIGHT}px` }}
              className="text-meta border-b border-(--border) pr-(--space-2) text-right text-(--text-muted)"
            >
              {hour === 0 ? "" : `${String(hour).padStart(2, "0")}:00`}
            </div>
          ))}
        </div>
        {days.map((day) => {
          const firings = assignWeekLanes(byDay.get(dayKey(day)) ?? []);
          const today = isSameDay(day, now);
          return (
            <div
              key={day.toISOString()}
              data-testid="calendar-week-day"
              data-today={today ? "true" : undefined}
              style={{ height: `${DAY_HEIGHT}px` }}
              className="relative border-l border-(--border)"
            >
              {firings.map(({ occurrence, lane, lanes }) => (
                <button
                  key={`${occurrence.binding_id}-${occurrence.dtstart}`}
                  type="button"
                  title={`${occurrence.scheduler_name} · ${formatBlockTime(new Date(occurrence.dtstart))}`}
                  onClick={() => onSelect(occurrence)}
                  className="absolute flex flex-col gap-px overflow-hidden rounded-(--radius-control) px-(--space-2) py-0.5 text-left text-(--on-accent) hover:opacity-90"
                  style={{
                    top: `${occurrenceTop(occurrence.dtstart)}px`,
                    left: `calc(var(--space-1) + (100% - 2 * var(--space-1)) * ${lane} / ${lanes})`,
                    width: `calc((100% - 2 * var(--space-1)) / ${lanes})`,
                    height: `${HOUR_HEIGHT}px`,
                    backgroundColor: occurrence.kind === "past" ? PAST_BLOCK : occurrence.scheduler_color,
                  }}
                >
                  <span className="text-meta tabular-nums opacity-90">
                    {formatBlockTime(new Date(occurrence.dtstart))}
                  </span>
                  <span className="text-meta truncate font-medium">{occurrence.scheduler_name}</span>
                </button>
              ))}
              {today ? (
                <div
                  aria-hidden="true"
                  data-testid="calendar-now-line"
                  className="pointer-events-none absolute inset-x-0 border-t-2 border-(--danger)"
                  style={{ top: `${lineTop}px` }}
                />
              ) : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}
