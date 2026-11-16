// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dependency shaping for the timeline (parity/specs/issues.md §20/§23,
// ISS-237). Pure geometry and edge building: which rows have a bar, where
// an arrow starts and ends, and which relation rows count as a finish-to-
// start dependency. The layer component and its read hook consume these;
// nothing here touches React, the network or the DOM.

import type { IssueRelations, RelatedIssue } from "@pidash/api-client";

import { barBox, type GanttBarBox, type GanttRow } from "./rows.js";
import type { GanttRange, GanttZoom } from "./scale.js";

/** Vertical breathing room inside a lane before an arrow turns. */
const ARROW_TURN_INSET = 11;
/** Distance the arrow keeps from the target bar's left edge. */
const ARROW_ENTRY_GAP = 2;
/** Radius of the rounded corners on the elbow. */
export const ARROW_CORNER_RADIUS = 6;
/** Marker id the layer paints its arrowheads with. */
export const ARROW_MARKER_ID = "gantt-dependency-arrow";

/** One drawn dependency: the predecessor finishes before the successor starts. */
export interface DependencyEdge {
  /** The item that must finish first. */
  fromId: string;
  /** The item that waits. */
  toId: string;
}

/** The relation type a finish-to-start edge is stored as, from the waiter's side. */
export const DEPENDENCY_RELATION_TYPE = "finish_after";

/**
 * Dependency rows a timeline may draw: a row without a bar cannot carry
 * an arrow end, and an unconfirmed create has no server identity yet.
 */
export function dependencyEligibleIds(rows: readonly { id: string; unconfirmed: boolean }[]): Set<string> {
  return new Set(rows.filter((row) => !row.unconfirmed).map((row) => row.id));
}

/**
 * Edges from one item's relations: every `finish_after` row is a
 * predecessor of this item. The list is unfiltered; the caller keeps the
 * ones whose other end the timeline draws (see `drawableEdges`).
 */
export function edgesOf(toId: string, relations: Pick<IssueRelations, "finish_after"> | undefined): DependencyEdge[] {
  if (relations === undefined) return [];
  return relations.finish_after.map((row: Pick<RelatedIssue, "id">) => ({ fromId: row.id, toId }));
}

/** Merge many per-item reads into one deduplicated edge list. */
export function mergeEdges(lists: readonly DependencyEdge[][]): DependencyEdge[] {
  const seen = new Set<string>();
  const edges: DependencyEdge[] = [];
  for (const list of lists) {
    for (const edge of list) {
      if (edge.fromId === edge.toId) continue;
      const key = `${edge.fromId}->${edge.toId}`;
      if (seen.has(key)) continue;
      seen.add(key);
      edges.push(edge);
    }
  }
  return edges;
}

/** What the arrow needs to know about one end. */
export interface DependencyEnd {
  box: GanttBarBox;
  laneTop: number;
  laneHeight: number;
}

/** The path an arrow takes between two bars, in strip coordinates. */
export function dependencyPath(from: DependencyEnd, to: DependencyEnd): string {
  const fromX = from.box.left + from.box.width;
  const fromY = from.laneTop + from.laneHeight / 2;
  const toX = to.box.left - ARROW_ENTRY_GAP;
  const toY = to.laneTop + to.laneHeight / 2;
  const sameLane = Math.abs(toY - fromY) < 1;
  // A forward link has room to turn in the gutter between the bars; a
  // backward (or overlapping) link drops beneath both lanes first.
  const forward = toX >= fromX + ARROW_CORNER_RADIUS * 2;
  if (forward) {
    if (sameLane) return `M ${round(fromX)} ${round(fromY)} L ${round(toX)} ${round(toY)}`;
    const midX = fromX + Math.max(ARROW_CORNER_RADIUS, (toX - fromX) / 2);
    const dir = toY > fromY ? 1 : -1;
    return [
      `M ${round(fromX)} ${round(fromY)}`,
      `L ${round(midX - ARROW_CORNER_RADIUS)} ${round(fromY)}`,
      `Q ${round(midX)} ${round(fromY)} ${round(midX)} ${round(fromY + dir * ARROW_CORNER_RADIUS)}`,
      `L ${round(midX)} ${round(toY - dir * ARROW_CORNER_RADIUS)}`,
      `Q ${round(midX)} ${round(toY)} ${round(midX + ARROW_CORNER_RADIUS)} ${round(toY)}`,
      `L ${round(toX)} ${round(toY)}`,
    ].join(" ");
  }
  const below = Math.max(from.laneTop + from.laneHeight, to.laneTop + to.laneHeight) - ARROW_TURN_INSET;
  return [
    `M ${round(fromX)} ${round(fromY)}`,
    `L ${round(fromX)} ${round(below - ARROW_CORNER_RADIUS)}`,
    `Q ${round(fromX)} ${round(below)} ${round(fromX - ARROW_CORNER_RADIUS)} ${round(below)}`,
    `L ${round(toX + ARROW_CORNER_RADIUS)} ${round(below)}`,
    `Q ${round(toX)} ${round(below)} ${round(toX)} ${round(below - ARROW_CORNER_RADIUS)}`,
    `L ${round(toX)} ${round(toY)}`,
  ].join(" ");
}

function round(value: number): number {
  return Math.round(value * 10) / 10;
}

/** The bounding box of every drawn arrow, so the SVG covers the strip. */
export function layerHeight(rows: readonly { unconfirmed: boolean }[], rowHeight: number): number {
  return rows.length * rowHeight;
}

/** Lane centre for one row index. */
export function laneCentre(rowIndex: number, rowHeight: number): number {
  return rowIndex * rowHeight + rowHeight / 2;
}

/**
 * The drawn end of every row that has a bar, in strip coordinates: its
 * box plus the lane it sits in. Rows without a bar (undated, or a create
 * still awaiting the server) carry no end, so no arrow can point at them.
 */
export function dependencyEnds(input: {
  rows: readonly GanttRow[];
  range: GanttRange;
  zoom: GanttZoom;
  rowHeight: number;
}): Map<string, DependencyEnd> {
  const ends = new Map<string, DependencyEnd>();
  input.rows.forEach((row, index) => {
    const box = barBox(row, input.range, input.zoom);
    if (box === null) return;
    ends.set(row.id, { box, laneTop: index * input.rowHeight, laneHeight: input.rowHeight });
  });
  return ends;
}

/** Edges worth drawing between the ends this timeline knows about. */
export function drawableEdges(
  edges: readonly DependencyEdge[],
  ends: ReadonlyMap<string, DependencyEnd>
): DependencyEdge[] {
  return edges.filter((edge) => ends.has(edge.fromId) && ends.has(edge.toId));
}
