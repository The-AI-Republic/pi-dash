// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Calendar grid (parity/specs/issues.md §4, ISS-021/023). A sticky
// weekday header follows the profile's first day, then one row per
// visible week. Columns are seven with weekends shown and five without.

import type { IssueListItem } from "@pidash/api-client";
import * as React from "react";

import { CalendarDay } from "./CalendarDay.js";
import { scrollContainerTowardPointer } from "./autoScroll.js";
import type { CalendarCell, CalendarWeek } from "./model.js";

export interface CalendarGridProps {
  weeks: readonly CalendarWeek[];
  weekdayLabels: readonly string[];
  rowsByDay: ReadonlyMap<string, IssueListItem[]>;
  totalsByDay: ReadonlyMap<string, number>;
  extraRowsByDay: ReadonlyMap<string, IssueListItem[]>;
  loadingDay: string | null;
  projectIdentifier: string;
  stateColors: ReadonlyMap<string, string>;
  highlightedId: string | null;
  narrow: boolean;
  canEdit: boolean;
  canAddExisting: boolean;
  selectedDay: string | null;
  dropTarget: string | null;
  dragging: boolean;
  onSelect: (day: string) => void;
  onDrop: (day: string) => void;
  onDragOver: (day: string) => void;
  onOpen: (issue: IssueListItem) => void;
  onEdit: (issue: IssueListItem) => void;
  onDelete: (issue: IssueListItem) => void;
  onDragStart: (issue: IssueListItem, event: React.PointerEvent<HTMLElement>) => void;
  onQuickAdd: (day: string, title: string) => void;
  onAddExisting: (day: string) => void;
  onLoadMore: (day: string) => void;
}

function Day({ cell, props }: { cell: CalendarCell; props: CalendarGridProps }): React.ReactElement {
  return (
    <CalendarDay
      cell={cell}
      rows={props.rowsByDay.get(cell.iso) ?? []}
      totalRows={props.totalsByDay.get(cell.iso) ?? props.rowsByDay.get(cell.iso)?.length ?? 0}
      projectIdentifier={props.projectIdentifier}
      stateColors={props.stateColors}
      highlightedId={props.highlightedId}
      narrow={props.narrow}
      canEdit={props.canEdit}
      canAddExisting={props.canAddExisting}
      selected={props.selectedDay === cell.iso}
      isDropTarget={props.dropTarget === cell.iso}
      onSelect={props.onSelect}
      onDrop={props.onDrop}
      onDragOver={props.onDragOver}
      onOpen={props.onOpen}
      onEdit={props.onEdit}
      onDelete={props.onDelete}
      onDragStart={props.onDragStart}
      onQuickAdd={props.onQuickAdd}
      onAddExisting={props.onAddExisting}
      onLoadMore={props.onLoadMore}
      extraRows={props.extraRowsByDay.get(cell.iso) ?? []}
      loadingMore={props.loadingDay === cell.iso}
    />
  );
}

export function CalendarGrid(props: CalendarGridProps): React.ReactElement {
  const columns = props.weeks[0]?.length ?? 7;
  const scrollRef = React.useRef<HTMLDivElement | null>(null);

  function handlePointerMove(event: React.PointerEvent<HTMLDivElement>): void {
    if (!props.dragging) return;
    const container = scrollRef.current;
    if (container === null) return;
    scrollContainerTowardPointer(container, event.clientY);
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="sticky top-0 z-10 bg-(--bg)">
        <div className="flex h-11 border-b border-(--border)">
          {props.weekdayLabels.slice(0, columns).map((label) => (
            <div
              key={label}
              className="text-caption flex h-11 min-w-0 flex-1 items-center px-(--space-2) font-medium text-(--text-muted)"
            >
              {label}
            </div>
          ))}
        </div>
      </div>
      <div
        ref={scrollRef}
        data-testid="calendar-scroll"
        className="min-h-0 flex-1 overflow-y-auto"
        onPointerMove={handlePointerMove}
      >
        <div className="grid h-full auto-rows-fr" style={{ gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` }}>
          {props.weeks.flat().map((cell) => (
            <Day key={cell.iso} cell={cell} props={props} />
          ))}
        </div>
      </div>
    </div>
  );
}
