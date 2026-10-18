// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline sidebar (parity/specs/issues.md §6, ISS-049). The sticky list
// beside the chart: one link per row with the short key ahead of the name
// and, for fully dated rows, a trailing day count. Rows scroll together
// with the chart's vertical axis, are windowed to the visible slice, and
// mark the hovered and peeked row. The href is absolute (workspace browse
// route) so Cmd/middle-click and copy-link land on the work item; a plain
// click opens the row through the layout instead.

import { cn } from "@pidash/kit";
import * as React from "react";

import { ROW_HEIGHT, durationLabel, type GanttRow } from "./rows.js";

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
  /** Virtualization window (row indices) shared with the chart lanes. */
  windowStart?: number;
  windowEnd?: number;
}

interface SidebarRowProps {
  row: GanttRow;
  workspaceSlug: string;
  peeked: boolean;
  hovered: boolean;
  onHover: (id: string | null) => void;
  onOpenRow: (row: GanttRow) => void;
}

export function SidebarRowItem({
  row,
  workspaceSlug,
  peeked,
  hovered,
  onHover,
  onOpenRow,
}: SidebarRowProps): React.ReactElement {
  const duration = durationLabel(row);
  return (
    <div
      data-testid="gantt-sidebar-row"
      data-row-id={row.id}
      style={{ height: ROW_HEIGHT }}
      onMouseEnter={() => onHover(row.id)}
      onMouseLeave={() => onHover(null)}
      className={cn(
        "group flex items-center gap-2 border-b border-(--border) px-(--space-3)",
        hovered && "bg-(--subtle)",
        peeked && "bg-(--accent-subtle)"
      )}
    >
      <a
        id={`issue-${row.id}`}
        href={`/${workspaceSlug}/browse/${row.identifier}`}
        aria-label={row.name}
        data-archived={row.archived ? "true" : undefined}
        data-peeked={peeked ? "true" : undefined}
        onClick={(event) => {
          if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
          event.preventDefault();
          onOpenRow(row);
        }}
        className={cn(
          "flex min-w-0 flex-1 items-center gap-(--space-2) truncate rounded-(--radius-control) px-(--space-1) py-(--space-1) text-left",
          "text-body text-(--text) hover:underline focus-visible:outline-none"
        )}
      >
        <span className="text-caption shrink-0 font-[family-name:var(--font-mono)] text-(--text-muted)">
          {row.identifier}
        </span>
        <span className="min-w-0 flex-1 truncate">{row.name}</span>
      </a>
      {duration !== null ? (
        <span data-testid="gantt-row-duration" className="text-caption shrink-0 text-(--text-muted)">
          {duration}
        </span>
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
  windowStart = 0,
  windowEnd = rows.length,
}: GanttSidebarProps): React.ReactElement {
  const visible = rows.slice(windowStart, windowEnd);
  return (
    <div id="gantt-sidebar" data-testid="gantt-sidebar">
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
        />
      ))}
      <div style={{ height: Math.max(0, rows.length - windowEnd) * ROW_HEIGHT }} aria-hidden="true" />
      {loadingMore ? (
        <div data-testid="gantt-load-more" className="flex h-11 items-center px-(--space-3)">
          <span aria-hidden="true" className="h-4 w-full animate-pulse rounded-(--radius-control) bg-(--subtle)" />
        </div>
      ) : null}
    </div>
  );
}
