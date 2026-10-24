// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board drag gesture (parity/specs/issues.md §5, ISS-037–040, ISS-043).
// A card press that travels past the threshold hands its card here; the
// hook then tracks the pointer on the whole document (a real drag leaves
// the card, so card-local listeners would go quiet), resolves the drop
// target under the cursor, and releases to the board's commit. While the
// drag holds — moving or still — a paced loop scrolls the board and the
// hovered column toward the pointer's edge zones.

import * as React from "react";

import type { IssueListItem } from "@pidash/api-client";

import { BOARD_AUTOSCROLL_INTERVAL_MS, scrollNodeTowardPoint } from "./autoScroll.js";
import { boardDropAtPoint, type BoardDropPoint } from "./dropTarget.js";

/** The card a drag carries, with its seat on the board. */
export interface DraggedCard {
  issue: IssueListItem;
  groupKey: string;
  laneKey: string | null;
  /** The card's index inside its own cell before the move. */
  sourceIndex: number;
}

/**
 * Track one board drag. The board starts it from a card press and commits
 * the release; the hook owns the in-flight target, the auto-scroll loop,
 * and the document listeners between them.
 */
export function useBoardDrag(
  boardRef: React.RefObject<HTMLElement | null>,
  onDrop: (dragged: DraggedCard, target: BoardDropPoint | null) => void
): {
  dragged: DraggedCard | null;
  target: BoardDropPoint | null;
  startDrag: (dragged: DraggedCard) => void;
} {
  const [dragged, setDragged] = React.useState<DraggedCard | null>(null);
  const [target, setTarget] = React.useState<BoardDropPoint | null>(null);
  const draggedRef = React.useRef<DraggedCard | null>(null);
  const targetRef = React.useRef<BoardDropPoint | null>(null);
  const pointRef = React.useRef<{ x: number; y: number } | null>(null);
  const dropRef = React.useRef(onDrop);
  dropRef.current = onDrop;

  const scrollToward = React.useCallback(
    (point: { x: number; y: number }) => {
      const board = boardRef.current;
      if (board === null) return;
      scrollNodeTowardPoint(board, point.x, point.y, { horizontal: true, vertical: true });
      const columnKey = targetRef.current?.columnKey;
      if (columnKey === null || columnKey === undefined) return;
      const body = board.querySelector(`[data-column-key="${columnKey}"] [data-testid="kanban-column-body"]`);
      if (body instanceof HTMLElement) {
        scrollNodeTowardPoint(body, point.x, point.y, { horizontal: false, vertical: true });
      }
    },
    [boardRef]
  );

  function startDrag(next: DraggedCard): void {
    draggedRef.current = next;
    targetRef.current = null;
    pointRef.current = null;
    setTarget(null);
    setDragged(next);
  }

  React.useEffect(() => {
    // Effects never run without a DOM, so the document is always here.
    if (dragged === null) return;
    const doc = globalThis.document;
    const onMove = (event: PointerEvent): void => {
      pointRef.current = { x: event.clientX, y: event.clientY };
      const point = boardDropAtPoint(doc, event.clientX, event.clientY);
      targetRef.current = point;
      setTarget(point);
      scrollToward({ x: event.clientX, y: event.clientY });
    };
    const onUp = (event: PointerEvent): void => {
      const point = boardDropAtPoint(doc, event.clientX, event.clientY);
      const done = draggedRef.current;
      draggedRef.current = null;
      targetRef.current = null;
      pointRef.current = null;
      setDragged(null);
      setTarget(null);
      if (done !== null) dropRef.current(done, point);
    };
    const onCancel = (): void => {
      draggedRef.current = null;
      targetRef.current = null;
      pointRef.current = null;
      setDragged(null);
      setTarget(null);
    };
    doc.addEventListener("pointermove", onMove);
    doc.addEventListener("pointerup", onUp);
    doc.addEventListener("pointercancel", onCancel);
    const timer = globalThis.setInterval(() => {
      const point = pointRef.current;
      if (point !== null) scrollToward(point);
    }, BOARD_AUTOSCROLL_INTERVAL_MS);
    return () => {
      doc.removeEventListener("pointermove", onMove);
      doc.removeEventListener("pointerup", onUp);
      doc.removeEventListener("pointercancel", onCancel);
      globalThis.clearInterval(timer);
    };
  }, [dragged, scrollToward]);

  return { dragged, target, startDrag };
}
