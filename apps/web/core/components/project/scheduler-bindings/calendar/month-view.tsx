/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useMemo } from "react";
import type { ISchedulerOccurrence } from "@pi-dash/services";
import { cn } from "@pi-dash/utils";
import { formatTime, formatWeekday, isSameDay, isToday, monthGridDays } from "./date-helpers";

type Props = {
  viewDate: Date;
  occurrences: ISchedulerOccurrence[];
  onSelectOccurrence: (occurrence: ISchedulerOccurrence) => void;
};

// Threshold above which a day's blocks are rolled up into "Nx" form. Decisions
// doc §8 — keeps month view readable for fast-firing bindings (e.g. */5).
const ROLLUP_THRESHOLD = 50;

/**
 * Month view: 6-week grid, each day shows up to N stacked blocks. Past
 * occurrences are grey; future ones use the scheduler's color.
 */
export function SchedulerMonthView({ viewDate, occurrences, onSelectOccurrence }: Props) {
  const days = useMemo(() => monthGridDays(viewDate), [viewDate]);
  const occurrencesByDay = useMemo(() => groupByDay(occurrences), [occurrences]);
  const now = new Date();

  // Sunday-rooted weekday header (Sun..Sat) using the first row of the grid
  // so the labels respect the user's locale.
  const weekdays = days.slice(0, 7);

  return (
    <div className="flex h-full w-full flex-col">
      <div className="grid grid-cols-7 border-b border-subtle">
        {weekdays.map((d) => (
          <div key={d.toISOString()} className="px-2 py-2 text-12 font-medium text-secondary uppercase">
            {formatWeekday(d)}
          </div>
        ))}
      </div>
      <div className="grid flex-1 auto-rows-fr grid-cols-7">
        {days.map((d) => {
          const key = d.toISOString().slice(0, 10);
          const blocks = occurrencesByDay.get(key) ?? [];
          const isInMonth = d.getMonth() === viewDate.getMonth();
          return (
            <DayCell
              key={key}
              date={d}
              isInMonth={isInMonth}
              isCurrentDay={isToday(d)}
              blocks={blocks}
              now={now}
              onSelectOccurrence={onSelectOccurrence}
            />
          );
        })}
      </div>
    </div>
  );
}

type DayCellProps = {
  date: Date;
  isInMonth: boolean;
  isCurrentDay: boolean;
  blocks: ISchedulerOccurrence[];
  now: Date;
  onSelectOccurrence: (o: ISchedulerOccurrence) => void;
};

function DayCell({ date, isInMonth, isCurrentDay, blocks, now, onSelectOccurrence }: DayCellProps) {
  // Density rollup — see decisions doc §8.
  const showRollup = blocks.length > ROLLUP_THRESHOLD;
  const visibleBlocks = showRollup ? [] : blocks.slice(0, 4);
  const overflow = blocks.length - visibleBlocks.length;

  return (
    <div
      className={cn(
        "flex min-h-[6rem] flex-col gap-0.5 border-r border-b border-subtle p-1.5",
        !isInMonth && "bg-layer-0/50"
      )}
    >
      <div className="flex items-center justify-between">
        <span
          className={cn(
            "text-12 font-medium",
            isCurrentDay
              ? "bg-primary text-on-primary rounded-full px-1.5 py-0.5"
              : isInMonth
                ? "text-primary"
                : "text-tertiary"
          )}
        >
          {date.getDate()}
        </span>
      </div>

      {showRollup && (
        <button
          type="button"
          onClick={() => onSelectOccurrence(blocks[0])}
          className="rounded-sm bg-layer-1 px-1.5 py-0.5 text-left text-12 text-primary hover:bg-layer-2"
        >
          {blocks.length}× {blocks[0].scheduler_name}
        </button>
      )}

      {!showRollup &&
        visibleBlocks.map((o) => {
          const isPast = new Date(o.dtstart) < now;
          return (
            <button
              type="button"
              key={`${o.binding_id}:${o.dtstart}`}
              onClick={() => onSelectOccurrence(o)}
              className="flex items-center gap-1 truncate rounded-sm px-1.5 py-0.5 text-left text-12 hover:opacity-80"
              style={{
                backgroundColor: isPast ? "#e5e7eb" : `${o.scheduler_color}22`,
                color: isPast ? "#6b7280" : o.scheduler_color,
                borderLeft: `3px solid ${isPast ? "#9ca3af" : o.scheduler_color}`,
              }}
              title={`${o.scheduler_name} — ${formatTime(new Date(o.dtstart))}${o.status ? ` (${o.status})` : ""}`}
            >
              <span className="font-medium">{formatTime(new Date(o.dtstart))}</span>
              <span className="truncate">{o.scheduler_name}</span>
            </button>
          );
        })}

      {!showRollup && overflow > 0 && (
        <button
          type="button"
          onClick={() => onSelectOccurrence(blocks[0])}
          className="rounded-sm px-1.5 py-0.5 text-left text-12 text-secondary hover:bg-layer-1"
        >
          + {overflow} more
        </button>
      )}
    </div>
  );
}

function groupByDay(occurrences: ISchedulerOccurrence[]): Map<string, ISchedulerOccurrence[]> {
  const out = new Map<string, ISchedulerOccurrence[]>();
  for (const o of occurrences) {
    const d = new Date(o.dtstart);
    const key = d.toISOString().slice(0, 10);
    const list = out.get(key) ?? [];
    list.push(o);
    out.set(key, list);
  }
  // Sort each day's blocks by time so the cell reads top-to-bottom.
  for (const list of out.values()) {
    list.sort((a, b) => a.dtstart.localeCompare(b.dtstart));
  }
  return out;
}

// Re-exported helper for tests / week-view.
export { isSameDay };
