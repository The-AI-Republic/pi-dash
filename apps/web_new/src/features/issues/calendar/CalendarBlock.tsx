// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Calendar issue block (parity/specs/issues.md §4, ISS-025). Each block
// is a peek-link anchor with a state-colour edge, identifier and title;
// hovering reveals a compact preview card, and a quick-actions trigger
// opens Edit/Delete. The block is the pointer-drag source on desktop;
// mobile tiles never mount it.

import { Menu } from "@pidash/kit";
import type { IssueListItem } from "@pidash/api-client";
import { Ellipsis } from "lucide-react";
import * as React from "react";

import { isTempRowId, rowDomId } from "../freshness/fresh.js";
import { previewPlacement, previewPlacementClasses, type PreviewPlacement } from "./placement.js";

export const CALENDAR_BLOCK_PREVIEW_DELAY_MS = 100;
/** Pointer travel before a press becomes a drag rather than a click. */
export const CALENDAR_DRAG_THRESHOLD_PX = 4;

export interface CalendarBlockProps {
  issue: IssueListItem;
  projectIdentifier: string;
  stateColor: string | null;
  highlighted: boolean;
  onOpen: (issue: IssueListItem) => void;
  onEdit: (issue: IssueListItem) => void;
  onDelete: (issue: IssueListItem) => void;
  onDragStart: (issue: IssueListItem, event: React.PointerEvent<HTMLElement>) => void;
  /** False on touch layouts and for read-only viewers. */
  draggable: boolean;
  /** False hides the hover quick actions for read-only viewers. */
  quickActions: boolean;
}

/** Project-scoped identifier, e.g. WEB-42. */
export function blockIdentifier(projectIdentifier: string, issue: IssueListItem): string {
  return `${projectIdentifier}-${issue.sequence_id}`;
}

export function CalendarBlock({
  issue,
  projectIdentifier,
  stateColor,
  highlighted,
  onOpen,
  onEdit,
  onDelete,
  onDragStart,
  draggable,
  quickActions,
}: CalendarBlockProps): React.ReactElement {
  const temp = isTempRowId(issue.id);
  const [previewOpen, setPreviewOpen] = React.useState(false);
  const [placement, setPlacement] = React.useState<PreviewPlacement>("bottom-start");
  const hoverTimer = React.useRef<ReturnType<typeof setTimeout> | null>(null);
  const anchorRef = React.useRef<HTMLAnchorElement | null>(null);
  const pressRef = React.useRef<{ x: number; y: number } | null>(null);
  const draggedRef = React.useRef(false);
  const identifier = blockIdentifier(projectIdentifier, issue);

  function openPreview(): void {
    const anchor = anchorRef.current;
    if (anchor !== null && typeof globalThis.window !== "undefined") {
      setPlacement(
        previewPlacement(anchor.getBoundingClientRect(), {
          width: globalThis.window.innerWidth,
          height: globalThis.window.innerHeight,
        })
      );
    }
    setPreviewOpen(true);
  }

  const clearHoverTimer = (): void => {
    if (hoverTimer.current !== null) {
      clearTimeout(hoverTimer.current);
      hoverTimer.current = null;
    }
  };

  React.useEffect(() => clearHoverTimer, []);

  return (
    <a
      ref={anchorRef}
      id={rowDomId(issue.id)}
      href={`/${projectIdentifier}/${issue.sequence_id}`}
      aria-label={temp ? `${issue.name}, saving` : identifier}
      data-highlight={highlighted ? "true" : undefined}
      data-temprow={temp ? "true" : undefined}
      className={`relative flex min-w-0 items-center gap-(--space-1) rounded-(--radius-control) border border-transparent bg-(--bg) px-(--space-1) py-(--space-1) text-left hover:border-(--border) ${highlighted ? "bg-(--accent-subtle) highlight" : ""}`}
      onPointerDown={(event) => {
        if (!draggable) return;
        draggedRef.current = false;
        pressRef.current = { x: event.clientX, y: event.clientY };
      }}
      onPointerMove={(event) => {
        const press = pressRef.current;
        if (press === null || !draggable) return;
        const dx = event.clientX - press.x;
        const dy = event.clientY - press.y;
        if (!Number.isFinite(dx) || !Number.isFinite(dy)) return;
        if (Math.hypot(dx, dy) < CALENDAR_DRAG_THRESHOLD_PX) return;
        pressRef.current = null;
        draggedRef.current = true;
        onDragStart(issue, event);
      }}
      onPointerUp={() => {
        pressRef.current = null;
      }}
      onPointerCancel={() => {
        pressRef.current = null;
      }}
      onClick={(event) => {
        event.preventDefault();
        if (draggedRef.current) {
          draggedRef.current = false;
          return;
        }
        onOpen(issue);
      }}
      onPointerEnter={() => {
        clearHoverTimer();
        hoverTimer.current = setTimeout(openPreview, CALENDAR_BLOCK_PREVIEW_DELAY_MS);
      }}
      onPointerLeave={() => {
        clearHoverTimer();
        setPreviewOpen(false);
      }}
      onFocus={openPreview}
      onBlur={() => setPreviewOpen(false)}
    >
      <span
        aria-hidden="true"
        className="h-3 w-1 shrink-0 rounded-full bg-(--text-muted)"
        style={stateColor === null ? undefined : { backgroundColor: stateColor }}
      />
      <button
        type="button"
        className="text-caption shrink-0 font-[family-name:var(--font-mono)] text-(--text-muted)"
        onClick={(event) => {
          event.stopPropagation();
        }}
      >
        {temp ? "…" : identifier}
      </button>
      <div className="min-w-0 flex-1 truncate text-13 text-[13px] text-(--text)">{issue.name}</div>
      {quickActions ? (
        <div
          className="shrink-0"
          onClick={(event) => event.stopPropagation()}
          onPointerDown={(event) => event.stopPropagation()}
          onPointerMove={(event) => event.stopPropagation()}
        >
          <Menu
            trigger={
              <button type="button" aria-label="Quick actions">
                <div className="cursor-pointer">
                  <Ellipsis size={13} strokeWidth={2} aria-hidden="true" />
                </div>
              </button>
            }
            items={[
              { label: "Edit", onSelect: () => onEdit(issue) },
              { label: "Delete", onSelect: () => onDelete(issue) },
            ]}
          />
        </div>
      ) : null}
      {previewOpen ? (
        <div
          className={`shadow-popover absolute z-30 w-72 space-y-2 rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-4) ${previewPlacementClasses(placement)}`}
          role="tooltip"
          data-placement={placement}
        >
          <p className="text-caption font-[family-name:var(--font-mono)] text-(--text-muted)">{identifier}</p>
          <p className="text-body font-medium text-(--text)">{issue.name}</p>
        </div>
      ) : null}
      {temp ? (
        <span aria-hidden="true" className="pointer-events-none absolute inset-0 animate-pulse bg-(--subtle)/60" />
      ) : null}
    </a>
  );
}
