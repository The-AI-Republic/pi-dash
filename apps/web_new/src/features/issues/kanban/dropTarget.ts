// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Resolve the board drop target under a pointer (parity/specs/issues.md
// §5, ISS-037–040). The drag never captures the pointer, so the target
// is read from the document at move and release time: the delete zone
// wins (it pins above the board), then the card under the cursor with
// its insertion edge, then the bare cell or column beneath. Pure over a
// minimal document shape, so the board and the tests share it.

export const KANBAN_DELETE_ZONE_SELECTOR = '[data-testid="kanban-delete-zone"]';
export const KANBAN_CARD_SELECTOR = '[data-testid="kanban-card"]';
export const KANBAN_CELL_SELECTOR = '[data-testid="kanban-cell"]';
export const KANBAN_COLUMN_SELECTOR = '[data-testid="kanban-column"]';

/** Where a drop lands relative to the target card. */
export type CardDropEdge = "above" | "below";

/** One resolved drop target. */
export type BoardDropPoint =
  | {
      kind: "card";
      columnKey: string;
      /** Destination lane key; null in flat mode. */
      laneKey: string | null;
      issueId: string;
      edge: CardDropEdge;
    }
  | { kind: "column"; columnKey: string; laneKey: null; issueId: null; edge: null }
  | {
      kind: "cell";
      columnKey: string;
      laneKey: string | null;
      issueId: null;
      edge: null;
    }
  | { kind: "delete"; columnKey: null; laneKey: null; issueId: null; edge: null };

interface PointElement {
  closest?: ((selectors: string) => PointElement | null) | undefined;
  getAttribute: (name: string) => string | null;
  getBoundingClientRect: () => { left: number; top: number; right: number; bottom: number };
}

interface PointDocument {
  elementFromPoint?: ((x: number, y: number) => PointElement | null) | undefined;
  querySelectorAll: (selectors: string) => ArrayLike<PointElement>;
}

function isPointInside(
  rect: { left: number; top: number; right: number; bottom: number },
  x: number,
  y: number
): boolean {
  if (rect.right <= rect.left || rect.bottom <= rect.top) return false;
  return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
}

function textOf(element: PointElement, name: string): string | null {
  const value = element.getAttribute(name);
  return value === null || value === "" ? null : value;
}

function cardEdge(card: PointElement, y: number): CardDropEdge {
  const rect = card.getBoundingClientRect();
  return y < (rect.top + rect.bottom) / 2 ? "above" : "below";
}

function cardPoint(card: PointElement, y: number): BoardDropPoint | null {
  const issueId = textOf(card, "data-issue-id");
  const columnKey = textOf(card, "data-group-key");
  if (issueId === null || columnKey === null) return null;
  return { kind: "card", columnKey, laneKey: textOf(card, "data-lane-key"), issueId, edge: cardEdge(card, y) };
}

function columnPoint(column: PointElement): BoardDropPoint | null {
  const columnKey = textOf(column, "data-column-key");
  if (columnKey === null) return null;
  return { kind: "column", columnKey, laneKey: null, issueId: null, edge: null };
}

function cellPoint(cell: PointElement): BoardDropPoint | null {
  const columnKey = textOf(cell, "data-column-key");
  if (columnKey === null) return null;
  return { kind: "cell", columnKey, laneKey: textOf(cell, "data-lane-key"), issueId: null, edge: null };
}

/**
 * Drop target under `(x, y)`, or null over empty board chrome. The
 * delete zone wins whenever the point lands inside its rect (it is
 * pointer-events-none, so hit-testing would fall through to the board
 * beneath it); otherwise the hit element climbs to the nearest card,
 * cell or column, and when the document cannot hit-test (or the hit
 * climbs nowhere), every remaining candidate is scanned by its rect.
 */
export function boardDropAtPoint(doc: PointDocument | null | undefined, x: number, y: number): BoardDropPoint | null {
  if (doc === null || doc === undefined) return null;
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
  for (const zone of Array.from(doc.querySelectorAll(KANBAN_DELETE_ZONE_SELECTOR))) {
    if (isPointInside(zone.getBoundingClientRect(), x, y)) {
      return { kind: "delete", columnKey: null, laneKey: null, issueId: null, edge: null };
    }
  }
  if (typeof doc.elementFromPoint === "function") {
    const hit = doc.elementFromPoint(x, y);
    const zone = hit?.closest?.(KANBAN_DELETE_ZONE_SELECTOR) ?? null;
    if (zone !== null) {
      return { kind: "delete", columnKey: null, laneKey: null, issueId: null, edge: null };
    }
    const card = hit?.closest?.(KANBAN_CARD_SELECTOR) ?? null;
    if (card !== null) {
      const point = cardPoint(card, y);
      if (point !== null) return point;
    }
    const cell = hit?.closest?.(KANBAN_CELL_SELECTOR) ?? null;
    if (cell !== null) {
      const point = cellPoint(cell);
      if (point !== null) return point;
    }
    const column = hit?.closest?.(KANBAN_COLUMN_SELECTOR) ?? null;
    if (column !== null) {
      const point = columnPoint(column);
      if (point !== null) return point;
    }
  }
  for (const card of Array.from(doc.querySelectorAll(KANBAN_CARD_SELECTOR))) {
    if (isPointInside(card.getBoundingClientRect(), x, y)) {
      const point = cardPoint(card, y);
      if (point !== null) return point;
    }
  }
  for (const cell of Array.from(doc.querySelectorAll(KANBAN_CELL_SELECTOR))) {
    if (isPointInside(cell.getBoundingClientRect(), x, y)) {
      const point = cellPoint(cell);
      if (point !== null) return point;
    }
  }
  for (const column of Array.from(doc.querySelectorAll(KANBAN_COLUMN_SELECTOR))) {
    if (isPointInside(column.getBoundingClientRect(), x, y)) {
      const point = columnPoint(column);
      if (point !== null) return point;
    }
  }
  return null;
}
