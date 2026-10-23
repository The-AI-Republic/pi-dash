// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline sidebar (parity/specs/issues.md §6, ISS-049/059/116). The
// sticky list beside the chart: one link per row with the short key ahead
// of the name and, for fully dated rows, a trailing day count. Rows
// scroll together with the chart's vertical axis, are windowed to the
// visible slice, and mark the hovered and peeked row. The href is absolute
// (workspace browse route) so Cmd/middle-click and copy-link land on the
// work item; a plain click opens the row through the layout instead. An
// unconfirmed create has no server key yet: its row pulses and is inert
// (no link, no peek), matching the list layout's temporary row
// (ISS-006/049). In manual sort the rows also drag to reorder (ISS-050):
// a pointer drag shows a drop line and the layout persists a
// neighbour-derived rank on release; outside manual sort the gesture is
// not mounted at all. Bulk builds add a selection checkbox per eligible
// row, and a pulsing sentinel trails the list while the server holds more.

import { cn } from "@pidash/kit";
import * as React from "react";

import { RowSelectionCheckbox, SELECTION_ROW_ATTR } from "../bulk/index.js";
import { DRAG_THRESHOLD_PX, type DropPlacement } from "./drag.js";
import { ROW_HEIGHT, durationLabel, type GanttRow } from "./rows.js";

/** Row half of the chart's bulk selection binding (ISS-116). */
export interface GanttRowSelection {
  isSelected: (id: string) => boolean;
  onRowToggle: (id: string, shiftKey: boolean) => void;
}

export interface GanttSidebarProps {
  rows: readonly GanttRow[];
  /** Workspace slug for the absolute browse hrefs (ISS-049). */
  workspaceSlug: string;
  /** Id of the row whose peek is open, for the selected outline. */
  peekedId: string | null;
  /** Hovered row id (the chart mirrors it on its lane). */
  hoveredId: string | null;
  onHover: (id: string | null) => void;
  onOpenRow: (row: GanttRow) => void;
  /** Rows still loading from the next page (pulsing placeholder). */
  loadingMore?: boolean;
  /** The server holds more rows than rendered (pulsing placeholder). */
  hasMore?: boolean;
  /** Bulk row toggles; null hides every checkbox (ISS-116). */
  selection?: GanttRowSelection | null;
  /** Virtualization window (row indices) shared with the chart lanes. */
  windowStart?: number;
  windowEnd?: number;
  /** Manual-sort reorder; absent outside manual order (ISS-050). */
  reorder?: ReorderBinding | null;
  /** Hover preview wrapper for row links (ISS-133). */
  renderPreview?:
    | ((row: GanttRow, trigger: React.ReactElement<Record<string, unknown>>) => React.ReactNode)
    | undefined;
}

/** The reorder seam the sidebar calls on drop. */
export interface ReorderBinding {
  onReorder: (sourceId: string, targetId: string, placement: DropPlacement) => void;
}

interface DragState {
  sourceId: string;
  targetId: string | null;
  placement: DropPlacement;
  /** Pointer travel so far, so a click is not treated as a drag. */
  travelled: boolean;
  startY: number;
}

interface SidebarRowProps {
  row: GanttRow;
  workspaceSlug: string;
  peeked: boolean;
  hovered: boolean;
  onHover: (id: string | null) => void;
  onOpenRow: (row: GanttRow) => void;
  drag: DragState | null;
  onDragStart: (event: React.MouseEvent, row: GanttRow) => void;
  onDragOver: (event: React.MouseEvent, row: GanttRow) => void;
  /** True for the click that ends a drag, which must not open the row. */
  suppressClick: () => boolean;
  /** Hover preview wrapper for the row link (ISS-133); absent leaves it plain. */
  renderPreview?:
    | ((row: GanttRow, trigger: React.ReactElement<Record<string, unknown>>) => React.ReactNode)
    | undefined;
  /** Bulk row toggle; null hides the checkbox (ISS-116). */
  selection?: GanttRowSelection | null;
}

export function SidebarRowItem({
  row,
  workspaceSlug,
  peeked,
  hovered,
  onHover,
  onOpenRow,
  drag,
  onDragStart,
  onDragOver,
  suppressClick,
  renderPreview,
  selection = null,
}: SidebarRowProps): React.ReactElement {
  const duration = durationLabel(row);
  const disabled = row.unconfirmed;
  const link = (
    <a
      id={`issue-${row.id}`}
      href={`/${workspaceSlug}/browse/${row.identifier}`}
      aria-label={row.name}
      data-archived={row.archived ? "true" : undefined}
      data-peeked={peeked ? "true" : undefined}
      onClick={(event) => {
        if (suppressClick()) {
          event.preventDefault();
          return;
        }
        if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
        event.preventDefault();
        onOpenRow(row);
      }}
      className={cn(
        "flex min-w-0 flex-1 items-center gap-(--space-2) truncate text-left",
        "hover:underline focus-visible:outline-none"
      )}
    >
      <span className="text-caption shrink-0 font-[family-name:var(--font-mono)] text-(--text-muted)">
        {row.identifier}
      </span>
      <span className="min-w-0 flex-1 truncate">{row.name}</span>
    </a>
  );
  const rowLink = renderPreview === undefined ? link : renderPreview(row, link);
  const dragging = drag?.sourceId === row.id;
  const dropBefore = drag !== null && drag.targetId === row.id && drag.placement === "before";
  const dropAfter = drag !== null && drag.targetId === row.id && drag.placement === "after";
  return (
    <div
      data-testid="gantt-sidebar-row"
      data-row-id={row.id}
      data-dragging={dragging ? "true" : undefined}
      {...{ [SELECTION_ROW_ATTR]: row.id }}
      style={{ height: ROW_HEIGHT }}
      onMouseEnter={() => onHover(row.id)}
      onMouseLeave={() => onHover(null)}
      onMouseDown={(event) => onDragStart(event, row)}
      onMouseMove={(event) => onDragOver(event, row)}
      className={cn(
        "group relative flex items-center gap-2 border-b border-(--border) px-(--space-3)",
        hovered && "bg-(--subtle)",
        peeked && "bg-(--accent-subtle)",
        dragging && "opacity-60"
      )}
    >
      {/* Epics never select and unconfirmed creates have no server identity for the bulk endpoints (ISS-116). */}
      {selection === null || row.unconfirmed || row.isEpic ? null : (
        <RowSelectionCheckbox
          checked={selection.isSelected(row.id)}
          label={`Select ${row.identifier}`}
          onChange={(event) => selection.onRowToggle(row.id, event.shiftKey)}
        />
      )}
      <div
        className={cn(
          "relative flex min-w-0 flex-1 items-center gap-(--space-2) rounded-(--radius-control) px-(--space-1) py-(--space-1)",
          "text-body text-(--text)"
        )}
      >
        {disabled ? (
          <>
            <span
              aria-label={`${row.name}, saving`}
              data-testid="gantt-row-unconfirmed"
              className="flex min-w-0 flex-1 items-center gap-(--space-2)"
            >
              <span className="text-caption shrink-0 font-[family-name:var(--font-mono)] text-(--text-muted)">…</span>
              <span className="min-w-0 flex-1 truncate">{row.name}</span>
            </span>
            <span
              aria-hidden="true"
              className="pointer-events-none absolute inset-0 animate-pulse rounded-(--radius-control) bg-(--subtle)/60"
            />
          </>
        ) : (
          rowLink
        )}
      </div>
      {duration !== null ? (
        <span data-testid="gantt-row-duration" className="text-caption shrink-0 text-(--text-muted)">
          {duration}
        </span>
      ) : null}
      {dropBefore ? (
        <span
          data-testid="gantt-drop-indicator"
          data-placement="before"
          aria-hidden="true"
          className="pointer-events-none absolute inset-x-0 top-0 h-0.5 bg-(--accent)"
        />
      ) : null}
      {dropAfter ? (
        <span
          data-testid="gantt-drop-indicator"
          data-placement="after"
          aria-hidden="true"
          className="pointer-events-none absolute inset-x-0 bottom-0 h-0.5 bg-(--accent)"
        />
      ) : null}
    </div>
  );
}

export function GanttSidebar({
  rows,
  workspaceSlug,
  peekedId,
  hoveredId,
  onHover,
  onOpenRow,
  loadingMore = false,
  hasMore = false,
  selection = null,
  windowStart = 0,
  windowEnd = rows.length,
  reorder = null,
  renderPreview,
}: GanttSidebarProps): React.ReactElement {
  const visible = rows.slice(windowStart, windowEnd);
  const [drag, setDrag] = React.useState<DragState | null>(null);
  const dragRef = React.useRef<DragState | null>(null);
  const dropped = React.useRef(false);
  const suppressClick = React.useCallback(() => dropped.current, []);

  const updateDrag = React.useCallback((next: DragState | null) => {
    dragRef.current = next;
    setDrag(next);
  }, []);

  const onDragStart = React.useCallback(
    (event: React.MouseEvent, row: GanttRow) => {
      if (reorder === null || row.unconfirmed || event.button !== 0) return;
      event.preventDefault();
      updateDrag({ sourceId: row.id, targetId: null, placement: "before", travelled: false, startY: event.clientY });
    },
    [reorder, updateDrag]
  );

  const onDragOver = React.useCallback(
    (event: React.MouseEvent, row: GanttRow) => {
      const current = dragRef.current;
      if (current === null || current.sourceId === row.id) return;
      const box = event.currentTarget.getBoundingClientRect();
      const placement: DropPlacement = event.clientY < box.top + box.height / 2 ? "before" : "after";
      const travelled = current.travelled || Math.abs(event.clientY - current.startY) >= DRAG_THRESHOLD_PX;
      if (travelled && (current.targetId !== row.id || current.placement !== placement || !current.travelled)) {
        dropped.current = true;
        updateDrag({ ...current, targetId: row.id, placement, travelled });
      }
    },
    [updateDrag]
  );

  // A drag only starts with a binding, so the listeners capture the one it
  // started under and never have to re-check the optional prop.
  React.useEffect(() => {
    if (drag === null || reorder === null) return;
    const binding = reorder;
    const finish = (commit: boolean) => {
      const current = dragRef.current;
      updateDrag(null);
      if (current === null || !current.travelled || current.targetId === null) return;
      if (!commit) return;
      binding.onReorder(current.sourceId, current.targetId, current.placement);
    };
    const onMove = (event: MouseEvent) => {
      const current = dragRef.current;
      if (current === null || current.travelled) return;
      if (Math.abs(event.clientY - current.startY) >= DRAG_THRESHOLD_PX) {
        dropped.current = true;
        updateDrag({ ...current, travelled: true });
      }
    };
    const onUp = () => finish(true);
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") finish(false);
    };
    globalThis.addEventListener("mousemove", onMove);
    globalThis.addEventListener("mouseup", onUp);
    globalThis.addEventListener("keydown", onKey);
    return () => {
      globalThis.removeEventListener("mousemove", onMove);
      globalThis.removeEventListener("mouseup", onUp);
      globalThis.removeEventListener("keydown", onKey);
    };
  }, [drag, reorder, updateDrag]);

  // A completed drag must not also activate the row's link.
  React.useEffect(() => {
    if (drag !== null) return;
    const reset = () => {
      globalThis.setTimeout(() => {
        dropped.current = false;
      }, 0);
    };
    globalThis.addEventListener("click", reset, { once: true });
    return () => globalThis.removeEventListener("click", reset);
  }, [drag]);

  return (
    <div id="gantt-sidebar" data-testid="gantt-sidebar" data-reorder={reorder !== null ? "true" : undefined}>
      <div style={{ height: windowStart * ROW_HEIGHT }} aria-hidden="true" />
      {visible.map((row) => (
        <SidebarRowItem
          key={row.id}
          row={row}
          workspaceSlug={workspaceSlug}
          peeked={row.id === peekedId}
          hovered={row.id === hoveredId}
          onHover={onHover}
          onOpenRow={onOpenRow}
          drag={drag}
          onDragStart={onDragStart}
          onDragOver={onDragOver}
          suppressClick={suppressClick}
          renderPreview={renderPreview}
          selection={selection}
        />
      ))}
      <div style={{ height: Math.max(0, rows.length - windowEnd) * ROW_HEIGHT }} aria-hidden="true" />
      {loadingMore || hasMore ? (
        <div data-testid="gantt-load-more" className="flex h-11 items-center px-(--space-3)">
          <span aria-hidden="true" className="h-4 w-full animate-pulse rounded-(--radius-control) bg-(--subtle)" />
        </div>
      ) : null}
    </div>
  );
}
