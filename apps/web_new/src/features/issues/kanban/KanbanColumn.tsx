// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One board column (parity/specs/issues.md §5, ISS-028–035, ISS-041–042).
// A column is its header plus a scrolling body of cards and the sticky
// quick-add. Collapsing folds it to the header alone at the narrow strip
// width, so the name reads vertically and the count stays visible while
// the cards unmount. Swimlane boards reuse the header row of columns and
// draw one cell per lane beneath it. While a drag hovers a column that
// cannot accept the card, a notice covers its body; under the cards a
// slot carries the pagination sentinel or the explicit load-more entry.

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
  /** Add control for the header; hidden while collapsed. */
  create?: React.ReactNode | undefined;
  /** Drop-disabled notice painted over the body while a drag hovers. */
  dropOverlayText?: string | null | undefined;
  /** Pagination sentinel, loaders or load-more entry under the cards. */
  afterRows?: React.ReactNode;
}

/** Notice covering a column body that cannot accept the dragged card. */
export function KanbanDropOverlay({ text }: { text: string }): React.ReactElement {
  return (
    <div
      data-testid="kanban-drop-overlay"
      className="absolute inset-0 z-10 flex items-center justify-center p-(--space-3)"
    >
      <div aria-hidden="true" className="absolute inset-0 bg-(--bg) opacity-80" />
      <span className="text-emphasis relative text-center text-(--text)">{text}</span>
    </div>
  );
}

/**
 * Scroll sentinel firing once its column end nears the viewport. The
 * board mounts it only while a next page exists and none is in flight,
 * so each mount re-arms it and concurrent loads never stack.
 */
export function KanbanColumnSentinel({ onVisible }: { onVisible: () => void }): React.ReactElement {
  const saved = React.useRef(onVisible);
  saved.current = onVisible;
  const observe = React.useCallback((node: HTMLDivElement | null) => {
    if (node === null || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) saved.current();
      },
      { rootMargin: "200px" }
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  return <div ref={observe} data-testid="kanban-column-sentinel" aria-hidden="true" />;
}

/** Explicit next-page entry for sub-grouped cells. */
export function KanbanLoadMore({ onLoad }: { onLoad: () => void }): React.ReactElement {
  return (
    <button
      type="button"
      data-testid="kanban-load-more"
      onClick={onLoad}
      className="text-body w-full px-(--space-3) py-(--space-2) text-center text-(--accent) hover:bg-(--subtle)"
    >
      Load more
    </button>
  );
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
  create,
  dropOverlayText,
  afterRows,
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
        <KanbanColumnHeader
          name={title}
          count={count}
          collapsed={collapsed}
          onToggleCollapse={onToggleCollapse}
          create={create}
        />
      </div>
      {collapsed ? null : (
        <>
          <div
            data-testid="kanban-column-body"
            className="relative flex min-h-0 flex-1 flex-col gap-(--space-2) overflow-y-auto p-(--space-2)"
          >
            {loading && rows.length === 0 ? <ColumnSkeleton /> : rows}
            {afterRows}
            {dropOverlayText === null || dropOverlayText === undefined ? null : (
              <KanbanDropOverlay text={dropOverlayText} />
            )}
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
  dropOverlayText,
  afterRows,
}: {
  laneKey: string;
  columnKey: string;
  collapsed: boolean;
  rows: readonly React.ReactNode[];
  footer?: React.ReactNode;
  /** Drop-disabled notice painted over the cell while a drag hovers. */
  dropOverlayText?: string | null | undefined;
  /** Loaders or the explicit load-more entry under the cards. */
  afterRows?: React.ReactNode;
}): React.ReactElement {
  return (
    <div
      data-testid="kanban-cell"
      data-lane-key={laneKey}
      data-column-key={columnKey}
      className="relative flex min-h-24 w-[350px] shrink-0 flex-col border-r border-b border-(--border)"
    >
      {collapsed ? null : (
        <>
          <div className="flex min-h-0 flex-1 flex-col gap-(--space-2) p-(--space-2)">
            {rows}
            {afterRows}
          </div>
          {footer}
          {dropOverlayText === null || dropOverlayText === undefined ? null : (
            <KanbanDropOverlay text={dropOverlayText} />
          )}
        </>
      )}
    </div>
  );
}
