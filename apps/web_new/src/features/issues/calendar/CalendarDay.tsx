// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One calendar day tile (parity/specs/issues.md §4, ISS-021/024/026/027).
// The desktop face renders blocks and a hover-revealed add control; the
// compact face selects the day and renders no blocks at all, so touch
// layouts cannot drag or hover one.

import { Menu } from "@pidash/kit";
import type { IssueListItem } from "@pidash/api-client";
import { Plus } from "lucide-react";
import * as React from "react";

import { CalendarBlock } from "./CalendarBlock.js";
import type { CalendarCell } from "./model.js";

export const CALENDAR_ADD_ITEM = "Add work item";
export const CALENDAR_ADD_EXISTING_ITEM = "Add existing work item";
export const CALENDAR_TITLE_PLACEHOLDER = "Work item Title";

export interface CalendarDayProps {
  cell: CalendarCell;
  rows: IssueListItem[];
  totalRows: number;
  workspaceSlug: string;
  projectIdentifier: string;
  stateColors: ReadonlyMap<string, string>;
  stateMeta: ReadonlyMap<string, { name: string; group: string }>;
  highlightedId: string | null;
  narrow: boolean;
  canEdit: boolean;
  /** False on touch layouts even when the viewer may edit. */
  canDrag: boolean;
  canAddExisting: boolean;
  selected: boolean;
  isDropTarget: boolean;
  onSelect: (day: string) => void;
  onDragOver: (day: string) => void;
  onOpen: (issue: IssueListItem) => void;
  onEdit: (issue: IssueListItem) => void;
  onDelete: (issue: IssueListItem) => void;
  onDragStart: (issue: IssueListItem, event: React.PointerEvent<HTMLElement>) => void;
  onQuickAdd: (day: string, title: string) => void;
  onAddExisting: (day: string) => void;
  onLoadMore: (day: string) => void;
  extraRows: IssueListItem[];
  loadingMore: boolean;
}

function stateColorFor(issue: IssueListItem, colors: ReadonlyMap<string, string>): string | null {
  return issue.state_id === null ? null : (colors.get(issue.state_id) ?? null);
}

function stateMetaFor(
  issue: IssueListItem,
  meta: ReadonlyMap<string, { name: string; group: string }>
): { name: string; group: string } | null {
  return issue.state_id === null ? null : (meta.get(issue.state_id) ?? null);
}

export function CalendarDay({
  cell,
  rows,
  totalRows,
  workspaceSlug,
  projectIdentifier,
  stateColors,
  stateMeta,
  highlightedId,
  narrow,
  canEdit,
  canDrag,
  canAddExisting,
  selected,
  isDropTarget,
  onSelect,
  onDragOver,
  onOpen,
  onEdit,
  onDelete,
  onDragStart,
  onQuickAdd,
  onAddExisting,
  onLoadMore,
  extraRows,
  loadingMore,
}: CalendarDayProps): React.ReactElement {
  const [adding, setAdding] = React.useState(false);
  const [title, setTitle] = React.useState("");
  const allRows = [...rows, ...extraRows.filter((extra) => !rows.some((row) => row.id === extra.id))];
  const hasMore = totalRows > allRows.length;
  // The desktop day number is decoration: it carries no click, so it never
  // takes a pointer. Should a crowded tile ever reach into a neighbouring
  // row, the strip still cannot swallow that row's controls (ISS-021).
  const headerClass = `pointer-events-none hidden flex-shrink-0 justify-end md:flex ${cell.inMonth ? "font-medium" : "text-(--text-muted)"}`;

  function submit(): void {
    const name = title.trim();
    if (name === "") return;
    onQuickAdd(cell.iso, name);
    setTitle("");
    setAdding(false);
  }

  return (
    <div
      className={`group relative flex h-full w-full flex-col border border-(--border) ${cell.weekend ? "bg-(--subtle)" : "bg-(--bg)"} ${cell.inMonth ? "" : "opacity-60"} ${isDropTarget ? "ring-2 ring-(--accent)" : ""}`}
      data-calendar-day={cell.iso}
      data-calendar-day-number={cell.day}
      data-in-month={cell.inMonth ? "true" : "false"}
      data-today={cell.isToday ? "true" : undefined}
      onPointerEnter={() => onDragOver(cell.iso)}
    >
      <div className={headerClass}>
        {cell.isToday ? (
          <span className="rounded-full bg-(--accent) px-(--space-1) text-(--on-accent)">{cell.day}</span>
        ) : (
          cell.day
        )}
      </div>
      <div
        className="text-meta mx-auto flex size-6 cursor-pointer items-center justify-center rounded-full font-medium md:hidden"
        role="button"
        tabIndex={0}
        aria-pressed={selected}
        data-today={cell.isToday ? "true" : undefined}
        onClick={() => onSelect(cell.iso)}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            onSelect(cell.iso);
          }
        }}
      >
        {cell.isToday ? (
          <span className="flex size-6 items-center justify-center rounded-full bg-(--accent) text-(--on-accent)">
            {cell.day}
          </span>
        ) : (
          <span className="text-(--text)">{cell.day}</span>
        )}
      </div>
      {narrow ? (
        allRows.length === 0 ? null : (
          <span aria-hidden="true" className="mx-auto mb-(--space-1) block size-1.5 rounded-full bg-(--text-muted)" />
        )
      ) : (
        <div className="flex min-h-0 flex-1 flex-col gap-(--space-1) px-(--space-1)">
          {allRows.map((issue) => (
            <CalendarBlock
              key={issue.id}
              issue={issue}
              workspaceSlug={workspaceSlug}
              projectIdentifier={projectIdentifier}
              stateColor={stateColorFor(issue, stateColors)}
              stateName={stateMetaFor(issue, stateMeta)?.name ?? null}
              stateGroup={stateMetaFor(issue, stateMeta)?.group ?? null}
              highlighted={issue.id === highlightedId}
              onOpen={onOpen}
              onEdit={onEdit}
              onDelete={onDelete}
              onDragStart={onDragStart}
              draggable={canDrag}
              quickActions={canEdit}
            />
          ))}
          {hasMore ? (
            <button
              type="button"
              className="text-caption self-start px-(--space-1) text-(--text-muted) hover:text-(--text)"
              disabled={loadingMore}
              onClick={() => onLoadMore(cell.iso)}
            >
              Load more
            </button>
          ) : null}
          {adding ? (
            <input
              autoFocus
              value={title}
              placeholder={CALENDAR_TITLE_PLACEHOLDER}
              aria-label="Work item title"
              className="text-caption w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-1) py-(--space-1) text-(--text) outline-none focus:border-(--accent)"
              onChange={(event) => setTitle(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") submit();
                if (event.key === "Escape") {
                  setAdding(false);
                  setTitle("");
                }
              }}
              onBlur={() => {
                if (title.trim() === "") setAdding(false);
              }}
            />
          ) : null}
          {canEdit ? (
            <Menu
              trigger={
                <button type="button" className="w-full text-left">
                  <div className="text-caption flex w-full items-center gap-(--space-1) px-(--space-1) text-(--text-muted) opacity-0 group-hover:opacity-100">
                    <Plus size={12} strokeWidth={2} aria-hidden="true" />
                    {CALENDAR_ADD_ITEM}
                  </div>
                </button>
              }
              items={[
                { label: CALENDAR_ADD_ITEM, onSelect: () => setAdding(true) },
                ...(canAddExisting
                  ? [{ label: CALENDAR_ADD_EXISTING_ITEM, onSelect: () => onAddExisting(cell.iso) }]
                  : []),
              ]}
            />
          ) : null}
        </div>
      )}
    </div>
  );
}
