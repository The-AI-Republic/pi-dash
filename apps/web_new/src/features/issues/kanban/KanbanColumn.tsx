// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One board column (parity/specs/issues.md §5, ISS-028–035). A column is
// its header plus a scrolling body of cards and the sticky quick-add.
// Collapsing folds it to the header alone at the narrow strip width, so
// the name reads vertically and the count stays visible while the cards
// unmount. Swimlane boards reuse the header row of columns and draw one
// cell per lane beneath it.

import { Skeleton, cn } from "@pidash/kit";
import * as React from "react";

import { KanbanColumnHeader } from "./KanbanColumnHeader.js";

/** Fixed column width the board scrolls horizontally through. */
export const COLUMN_WIDTH = 350;

export interface KanbanColumnProps {
  columnKey: string;
  title: string;
  count: number;
  /** Cards loaded for this column. */
  rows: readonly React.ReactNode[];
  /** Whether the column's first page is still loading. */
  loading: boolean;
  /** Flat columns fold through the header's control; lanes fold instead. */
  collapsed: boolean;
  onToggleCollapse: () => void;
  /** Sticky quick-add under the cards. */
  footer?: React.ReactNode;
}

/** Placeholder shells while a column's first page loads. */
export function ColumnSkeleton(): React.ReactElement {
  return (
    <div data-testid="kanban-column-loading" className="flex flex-col gap-(--space-2) p-(--space-3)">
      {Array.from({ length: 3 }, (_, index) => (
        <Skeleton key={index} height="64px" />
      ))}
    </div>
  );
}

export function KanbanColumn({
  columnKey,
  title,
  count,
  rows,
  loading,
  collapsed,
  onToggleCollapse,
  footer,
}: KanbanColumnProps): React.ReactElement {
  return (
    <section
      data-testid="kanban-column"
      data-column-key={columnKey}
      data-column-name={title}
      data-collapsed={collapsed ? "true" : undefined}
      className={cn("flex h-full shrink-0 flex-col border-r border-(--border)", collapsed ? "overflow-hidden" : "")}
      style={{ width: collapsed ? 44 : COLUMN_WIDTH }}
    >
      <div className={cn("sticky top-0 z-10 shrink-0", collapsed ? "flex-1" : "")}>
        <KanbanColumnHeader name={title} count={count} collapsed={collapsed} onToggleCollapse={onToggleCollapse} />
      </div>
      {collapsed ? null : (
        <>
          <div
            data-testid="kanban-column-body"
            className="flex min-h-0 flex-1 flex-col gap-(--space-2) overflow-y-auto p-(--space-2)"
          >
            {loading && rows.length === 0 ? <ColumnSkeleton /> : rows}
          </div>
          {footer}
        </>
      )}
    </section>
  );
}

/** One swimlane cell: a column's cards for a single lane. */
export function KanbanLaneColumn({
  laneKey,
  columnKey,
  collapsed,
  rows,
  footer,
}: {
  laneKey: string;
  columnKey: string;
  collapsed: boolean;
  rows: readonly React.ReactNode[];
  footer?: React.ReactNode;
}): React.ReactElement {
  return (
    <div
      data-testid="kanban-cell"
      data-lane-key={laneKey}
      data-column-key={columnKey}
      className="flex min-h-24 w-[350px] shrink-0 flex-col border-r border-b border-(--border)"
    >
      {collapsed ? null : (
        <>
          <div className="flex min-h-0 flex-1 flex-col gap-(--space-2) p-(--space-2)">{rows}</div>
          {footer}
        </>
      )}
    </div>
  );
}
