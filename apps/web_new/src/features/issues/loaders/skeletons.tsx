// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-layout loading skeletons (parity/specs/issues.md §1, ISS-004/ISS-005).
// Each layout shows its own placeholder while its first load runs; list and
// spreadsheet reuse the row skeletons for pagination, calendar its pulsing
// blocks plus the top reload bar. The list skeleton stands alone in the
// eager ListSkeleton leaf (the route pending component renders it).

import { Skeleton } from "@pidash/kit";
import * as React from "react";

import type { LayoutKind } from "../layouts/kinds.js";
import { ListSkeleton } from "./ListSkeleton.js";

const PLACEHOLDER_ROWS = 8;
const BOARD_COLUMNS = 3;
const BOARD_CARDS = 4;
const CALENDAR_CELLS = 35;
const GANTT_ROWS = 6;

function rows(count: number): number[] {
  return Array.from({ length: count }, (_, index) => index);
}

/** Spreadsheet placeholder: header plus body cell rows (ISS-004/005). */
export function SpreadsheetSkeleton(): React.ReactElement {
  return (
    <div data-testid="issues-skeleton-spreadsheet" className="flex h-full flex-col">
      <div className="flex h-11 items-center gap-(--space-3) border-b border-(--border) px-(--space-4)">
        <Skeleton width="140px" height="14px" />
        <Skeleton width="90px" height="14px" />
        <Skeleton width="90px" height="14px" />
        <Skeleton width="70px" height="14px" />
      </div>
      {rows(PLACEHOLDER_ROWS).map((index) => (
        <div
          key={index}
          data-skeleton-row="true"
          className="flex h-11 items-center gap-(--space-3) border-b border-(--border) px-(--space-4)"
        >
          <Skeleton width="180px" height="14px" />
          <Skeleton width="80px" />
          <Skeleton width="80px" />
          <Skeleton width="120px" />
        </div>
      ))}
    </div>
  );
}

/** Thin loading bar across the top of the calendar while it reloads (ISS-005). */
export function CalendarReloadBar(): React.ReactElement {
  return (
    <div data-testid="issues-calendar-reload-bar" className="h-0.5 w-full overflow-hidden bg-(--subtle)">
      <div className="h-full w-1/3 animate-pulse bg-(--accent)" />
    </div>
  );
}

/** Calendar placeholder: reload bar plus pulsing day blocks (ISS-004/005). */
export function CalendarSkeleton(): React.ReactElement {
  return (
    <div data-testid="issues-skeleton-calendar" className="flex h-full flex-col gap-(--space-3) p-(--space-4)">
      <CalendarReloadBar />
      <div className="grid grid-cols-7 gap-(--space-2)">
        {rows(CALENDAR_CELLS).map((index) => (
          <div
            key={index}
            data-skeleton-row="true"
            className="flex h-20 flex-col gap-(--space-1) rounded-(--radius-control) border border-(--border) p-(--space-2)"
          >
            <span className="h-4 w-2/3 animate-pulse rounded-(--radius-control) bg-(--subtle)" />
            <span className="h-4 w-1/2 animate-pulse rounded-(--radius-control) bg-(--subtle)" />
          </div>
        ))}
      </div>
    </div>
  );
}

/** Board placeholder: grouped columns with card blocks (ISS-004). */
export function KanbanSkeleton(): React.ReactElement {
  return (
    <div data-testid="issues-skeleton-kanban" className="flex h-full gap-(--space-3) p-(--space-4)">
      {rows(BOARD_COLUMNS).map((column) => (
        <div
          key={column}
          data-skeleton-row="true"
          className="flex w-72 shrink-0 flex-col gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--subtle) p-(--space-3)"
        >
          <Skeleton width="120px" height="14px" />
          {rows(BOARD_CARDS).map((card) => (
            <div
              key={card}
              className="flex flex-col gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) p-(--space-3)"
            >
              <Skeleton height="14px" />
              <Skeleton width="60%" />
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}

/** Timeline placeholder: sidebar rows beside chart bars (ISS-004). */
export function GanttSkeleton(): React.ReactElement {
  return (
    <div data-testid="issues-skeleton-gantt" className="flex h-full gap-(--space-4) p-(--space-4)">
      <div className="flex w-56 shrink-0 flex-col gap-(--space-3)">
        {rows(GANTT_ROWS).map((index) => (
          <Skeleton key={index} height="20px" />
        ))}
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-(--space-3)">
        {rows(GANTT_ROWS).map((index) => (
          <div key={index} data-skeleton-row="true" className="flex h-5 items-center">
            <span
              aria-hidden="true"
              style={{ width: `${30 + ((index * 13) % 45)}%`, marginLeft: `${(index * 17) % 30}%` }}
              className="block h-3 rounded-full bg-(--subtle)"
            />
          </div>
        ))}
      </div>
    </div>
  );
}

/** The skeleton placeholder for one layout kind. */
export function LayoutSkeleton({ kind }: { kind: LayoutKind }): React.ReactElement {
  switch (kind) {
    case "list":
      return <ListSkeleton />;
    case "spreadsheet":
      return <SpreadsheetSkeleton />;
    case "calendar":
      return <CalendarSkeleton />;
    case "kanban":
      return <KanbanSkeleton />;
    case "gantt_chart":
      return <GanttSkeleton />;
  }
}
