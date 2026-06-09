/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import type { ISchedulerOccurrence } from "@pi-dash/services";
import { cn } from "@pi-dash/utils";
import { formatDayHeader, formatTime, isToday, weekGridDays } from "./date-helpers";
import { getOccurrenceStyle } from "./occurrence-style";

type Props = {
  viewDate: Date;
  occurrences: ISchedulerOccurrence[];
  onSelectOccurrence: (occurrence: ISchedulerOccurrence) => void;
};

// Per decisions doc §5 — same as Google Calendar default. 24h × 48px = 1152px.
const HOUR_HEIGHT = 48;
const BLOCK_HEIGHT = 20;
const TIME_COL_WIDTH = 64;
const HOURS = Array.from({ length: 24 }, (_, i) => i);

/**
 * Time-axis week view: vertical hour ruler on the left, 7 day columns to
 * the right, each occurrence positioned by its minute-of-day. Net-new
 * visual primitive — not present in the existing issue calendar.
 *
 * Read-only — no drag-to-create / drag-to-reschedule in v1.
 */
export function SchedulerWeekView({ viewDate, occurrences, onSelectOccurrence }: Props) {
  const days = useMemo(() => weekGridDays(viewDate), [viewDate]);
  const byDay = useMemo(() => groupAndPositionByDay(days, occurrences), [days, occurrences]);

  const containerRef = useRef<HTMLDivElement | null>(null);

  // Scroll-snap: on mount, show today's current hour minus 1 (or 8am if no
  // "today" is in the visible week — gives morning bindings a head start).
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const now = new Date();
    const inThisWeek = days.some((d) => isToday(d));
    const hour = inThisWeek ? Math.max(0, now.getHours() - 1) : 8;
    el.scrollTop = hour * HOUR_HEIGHT;
  }, [days]);

  // ``now`` is captured per render. The minute-resolution refresh lives on
  // the <CurrentTimeLine /> child below so the per-minute tick doesn't
  // re-render every occurrence button (a 5000-block reconciliation per
  // minute while the tab is open).
  const now = useMemo(() => new Date(), []);

  return (
    <div className="flex h-full w-full flex-col">
      {/* Day-column header row, sticky at top */}
      <div className="flex border-b border-subtle">
        <div className="flex-shrink-0" style={{ width: TIME_COL_WIDTH }} />
        {days.map((d) => {
          const today = isToday(d);
          return (
            <div
              key={d.toISOString()}
              className={cn(
                "flex-1 border-l border-subtle px-2 py-2 text-12 font-medium",
                today ? "text-primary" : "text-secondary"
              )}
            >
              <div className="flex items-baseline gap-1">
                <span>{formatDayHeader(d)}</span>
                {today && <span className="bg-primary text-on-primary rounded-full px-1.5 py-0.5 text-11">Today</span>}
              </div>
            </div>
          );
        })}
      </div>

      {/* Scrollable hour grid */}
      <div ref={containerRef} className="flex flex-1 overflow-y-auto">
        {/* Time column */}
        <div className="flex-shrink-0" style={{ width: TIME_COL_WIDTH }}>
          {HOURS.map((h) => (
            <div
              key={`time-${h}`}
              className="relative pr-2 text-right text-11 text-tertiary"
              style={{ height: HOUR_HEIGHT }}
            >
              <span className="absolute -top-1.5 right-2">{formatHourLabel(h)}</span>
            </div>
          ))}
        </div>

        {/* Day columns */}
        {days.map((d, dayIdx) => (
          <DayColumn
            key={d.toISOString()}
            date={d}
            blocks={byDay[dayIdx]}
            now={now}
            onSelectOccurrence={onSelectOccurrence}
          />
        ))}
      </div>
    </div>
  );
}

type DayColumnProps = {
  date: Date;
  blocks: PositionedBlock[];
  /** Snapshot at parent-render time. Used for past/future color flip. */
  now: Date;
  onSelectOccurrence: (o: ISchedulerOccurrence) => void;
};

function DayColumn({ date, blocks, now, onSelectOccurrence }: DayColumnProps) {
  const dayIsToday = isToday(date);

  return (
    <div className="relative flex-1 border-l border-subtle" style={{ minHeight: 24 * HOUR_HEIGHT }}>
      {/* Hour grid lines */}
      {HOURS.map((h) => (
        <div key={`grid-${h}`} className="border-b border-dashed border-subtle" style={{ height: HOUR_HEIGHT }} />
      ))}

      {/* Current-time line is in its own component so its per-minute tick
          doesn't re-render the surrounding occurrence buttons. */}
      {dayIsToday && <CurrentTimeLine />}

      {/* Occurrence blocks */}
      {blocks.map((block) => {
        const widthPct = 100 / block.lane.total;
        const leftPct = widthPct * block.lane.index;
        return (
          <button
            type="button"
            key={`${block.occurrence.binding_id}:${block.occurrence.dtstart}`}
            onClick={() => onSelectOccurrence(block.occurrence)}
            className="absolute z-20 truncate rounded-sm px-1.5 text-left text-11 hover:opacity-80"
            style={{
              top: block.top,
              height: BLOCK_HEIGHT,
              left: `calc(${leftPct}% + 2px)`,
              width: `calc(${widthPct}% - 4px)`,
              ...getOccurrenceStyle(block.occurrence, now),
            }}
            title={`${block.occurrence.scheduler_name} — ${formatTime(
              new Date(block.occurrence.dtstart)
            )}${block.occurrence.status ? ` (${block.occurrence.status})` : ""}`}
          >
            <span className="font-medium">{formatTime(new Date(block.occurrence.dtstart))}</span>{" "}
            <span>{block.occurrence.scheduler_name}</span>
          </button>
        );
      })}
    </div>
  );
}

function formatHourLabel(h: number): string {
  const d = new Date();
  d.setHours(h, 0, 0, 0);
  return new Intl.DateTimeFormat(undefined, { hour: "numeric" }).format(d);
}

function CurrentTimeLine() {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 60 * 1000);
    return () => window.clearInterval(id);
  }, []);
  const top = ((now.getHours() * 60 + now.getMinutes()) / 60) * HOUR_HEIGHT;
  return (
    <div className="pointer-events-none absolute right-0 left-0 z-10 border-t-2 border-danger-strong" style={{ top }}>
      <div className="absolute -top-1.5 -left-1 size-3 rounded-full bg-danger-primary" />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Positioning + simple overlap lane assignment.
// ---------------------------------------------------------------------------

type PositionedBlock = {
  occurrence: ISchedulerOccurrence;
  top: number;
  lane: { index: number; total: number };
};

/**
 * Group occurrences by day, position them by minute-of-day, and assign each
 * a lane within its day for overlap rendering (Google Calendar-style
 * side-by-side at proportional widths).
 *
 * v1 algorithm: blocks that share the same minute go side-by-side at
 * 1/Nth width. Overlap detection beyond exact-minute matches (e.g. one
 * fires at 9:00 and the next at 9:00:30) is out of scope — point-in-time
 * triggers don't have duration, so the only collision we model is
 * same-minute.
 */
function groupAndPositionByDay(days: Date[], occurrences: ISchedulerOccurrence[]): PositionedBlock[][] {
  const out: PositionedBlock[][] = days.map(() => []);
  const byDay: ISchedulerOccurrence[][] = days.map(() => []);

  for (const o of occurrences) {
    const d = new Date(o.dtstart);
    const dayIdx = days.findIndex(
      (day) => day.getFullYear() === d.getFullYear() && day.getMonth() === d.getMonth() && day.getDate() === d.getDate()
    );
    if (dayIdx === -1) continue;
    byDay[dayIdx].push(o);
  }

  for (let i = 0; i < days.length; i++) {
    // sort() is fine on the per-day arrays we just constructed locally; the
    // tsconfig lib (ES2022) doesn't include toSorted yet.
    // eslint-disable-next-line unicorn/no-array-sort
    const list = byDay[i].sort((a, b) => a.dtstart.localeCompare(b.dtstart));
    // Same-minute grouping for lane assignment.
    const byMinute = new Map<string, ISchedulerOccurrence[]>();
    for (const o of list) {
      const d = new Date(o.dtstart);
      const key = `${d.getHours()}:${d.getMinutes()}`;
      const arr = byMinute.get(key) ?? [];
      arr.push(o);
      byMinute.set(key, arr);
    }
    for (const arr of byMinute.values()) {
      const total = arr.length;
      arr.forEach((occ, idx) => {
        const d = new Date(occ.dtstart);
        const top = ((d.getHours() * 60 + d.getMinutes()) / 60) * HOUR_HEIGHT;
        out[i].push({ occurrence: occ, top, lane: { index: idx, total } });
      });
    }
  }
  return out;
}
