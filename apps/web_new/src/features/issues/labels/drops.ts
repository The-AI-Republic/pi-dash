// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Label drag resolution (parity/specs/issues.md §19, ISS-229): dropping
// onto a top-level label nests as its child, dropping above or below
// reorders with a neighbor-derived ordering value, group sources
// reorder instead of nesting, drops onto a child land as its sibling,
// and self plus parent-onto-own-child drops refuse. Pure: the settings
// list measures rows and commits the resolved move.

import type { Label } from "@pidash/api-client";
import { compareLabelOrder } from "./tree.js";

/** Ordering step appended past the tail (and into a nested group). */
export const LABEL_ORDER_STEP = 10000;

/** Drop position: above a row, or past the last row. */
export interface LabelDropSlot {
  id: string;
  after: boolean;
}

/** One measured label row for drop resolution. */
export interface LabelDropSpan {
  id: string;
  top: number;
  bottom: number;
  left: number;
  right: number;
  /** Top-level rows accept nesting; child rows resolve to siblings. */
  nestable: boolean;
  /** Parent link of a child row; null for top-level rows. */
  parentId: string | null;
}

/** Attribute marking one measured row, carrying its id and parent. */
export const LABEL_SLOT_ATTRIBUTE = "data-label-slot";

/**
 * Measure the rows under a list root for drop resolution. Rows without
 * an id (still mounting) are skipped; a missing root measures nothing.
 */
export function readLabelDropSpans(root: Element | null | undefined): LabelDropSpan[] {
  const nodes =
    root === null || root === undefined ? [] : Array.from(root.querySelectorAll(`[${LABEL_SLOT_ATTRIBUTE}]`));
  const spans: LabelDropSpan[] = [];
  for (const node of nodes) {
    const element = node as HTMLElement;
    const id = element.dataset["labelId"] ?? "";
    if (id === "") continue;
    const rect = element.getBoundingClientRect();
    spans.push({
      id,
      top: rect.top,
      bottom: rect.bottom,
      left: rect.left,
      right: rect.right,
      nestable: element.dataset["nestable"] === "true",
      parentId: element.dataset["labelParent"] ?? null,
    });
  }
  return spans;
}

/** Live drop preview: a sibling slot, a nesting target, or nothing. */
export interface LabelDropPreview {
  slot: LabelDropSlot | null;
  into: string | null;
}

/** Empty preview: no marker lit. */
export const EMPTY_LABEL_DROP: LabelDropPreview = { slot: null, into: null };

/**
 * Sibling slot under the pointer: the first row whose middle the
 * pointer has not reached yet, or past the last row when it cleared
 * them all. The dragged row itself resolves to no slot.
 */
export function resolveLabelSlot(
  spans: readonly LabelDropSpan[],
  pointerY: number,
  sourceId: string
): LabelDropSlot | null {
  let last: LabelDropSpan | null = null;
  for (const span of spans) {
    if (pointerY < (span.top + span.bottom) / 2) {
      return span.id === sourceId ? null : { id: span.id, after: false };
    }
    last = span;
  }
  if (last === null || last.id === sourceId) return null;
  return { id: last.id, after: true };
}

/**
 * True when the pointer sits in the row's middle band: quarter-inset
 * on every side so edge releases keep their reorder meaning.
 * Degenerate (zero-area) rows never match.
 */
function inMiddleBand(span: LabelDropSpan, x: number, y: number): boolean {
  const width = span.right - span.left;
  const height = span.bottom - span.top;
  return (
    width > 0 &&
    height > 0 &&
    x >= span.left + width / 4 &&
    x <= span.right - width / 4 &&
    y >= span.top + height / 4 &&
    y <= span.bottom - height / 4
  );
}

/**
 * Nesting target under the pointer, or null. Only top-level rows
 * match, never the dragged row itself.
 */
export function resolveLabelInto(
  spans: readonly LabelDropSpan[],
  x: number,
  y: number,
  sourceId: string
): string | null {
  for (const span of spans) {
    if (span.nestable && span.id !== sourceId && inMiddleBand(span, x, y)) return span.id;
  }
  return null;
}

/**
 * Sibling slot for a drop over a child row: the whole row previews a
 * slot above it, and the last child's lower half previews a slot
 * below. Children never accept nesting, so the pointer never falls
 * through to the top-level rows behind them. The dragged row itself
 * resolves to no slot.
 */
export function resolveLabelChildSlot(
  spans: readonly LabelDropSpan[],
  x: number,
  y: number,
  sourceId: string
): LabelDropSlot | null {
  for (let index = 0; index < spans.length; index += 1) {
    const span = spans[index];
    if (span === undefined || span.nestable || span.id === sourceId) continue;
    if (x < span.left || x > span.right || y < span.top || y > span.bottom) continue;
    const isLastChild = !spans.slice(index + 1).some((later) => later.parentId === span.parentId);
    const below = isLastChild && y > (span.top + span.bottom) / 2;
    return { id: span.id, after: below };
  }
  return null;
}

/**
 * Effective parent link, mirroring the tree: missing, self, and
 * deeper-than-two links flatten to top level so drops resolve against
 * the rendered hierarchy.
 */
export function effectiveLabelParent(labels: readonly Label[], row: Label): string | null {
  const parent = row.parent;
  if (parent === null || parent === row.id) return null;
  const target = labels.find((candidate) => candidate.id === parent);
  if (target === undefined) return null;
  if (target.parent !== null && target.parent !== target.id && labels.some((row) => row.id === target.parent)) {
    return null;
  }
  return parent;
}

/** Effective children of one label, sorted by the ordering value. */
export function effectiveLabelChildren(labels: readonly Label[], id: string): Label[] {
  return labels.filter((row) => row.id !== id && effectiveLabelParent(labels, row) === id).sort(compareLabelOrder);
}

/**
 * Ordering value landing a row at its insertion slot with one write:
 * half of next at the head, the midpoint between neighbors inside,
 * past the edge neighbor at the tail. Null for an empty destination,
 * where the server assigns the order instead.
 */
export function neighborLabelOrder(rest: readonly Label[], insertion: number): number | null {
  const prev = rest[insertion - 1];
  const next = rest[insertion];
  if (prev !== undefined && next !== undefined) return (prev.sort_order + next.sort_order) / 2;
  if (next !== undefined) return next.sort_order / 2;
  if (prev !== undefined) return prev.sort_order + LABEL_ORDER_STEP;
  return null;
}

/** The single label write for a drop: the parent plus the order, if any. */
export interface LabelDropMove {
  parent: string | null;
  sortOrder?: number | undefined;
}

/**
 * Resolve a drop to its write, or null when it changes nothing. Nest
 * instructions convert by rule (child targets to sibling-above, group
 * sources to reorder-above); drops with no movement inside the same
 * parent skip with no call.
 */
export function resolveLabelDrop(args: {
  labels: readonly Label[];
  sourceId: string;
  slot: LabelDropSlot | null;
  into: string | null;
}): LabelDropMove | null {
  const labels = args.labels;
  const source = labels.find((row) => row.id === args.sourceId);
  if (source === undefined) return null;

  let slot = args.slot;
  const into = args.into;
  if (into !== null) {
    const target = labels.find((row) => row.id === into);
    if (target === undefined || target.id === source.id) return null;
    if (effectiveLabelParent(labels, target) === source.id) return null;
    if (effectiveLabelParent(labels, target) !== null || effectiveLabelChildren(labels, source.id).length > 0) {
      slot = { id: target.id, after: false };
    } else if (effectiveLabelParent(labels, source) === into) {
      return null;
    } else {
      const kids = effectiveLabelChildren(labels, into).filter((row) => row.id !== source.id);
      const last = kids[kids.length - 1];
      return last === undefined ? { parent: into } : { parent: into, sortOrder: last.sort_order + LABEL_ORDER_STEP };
    }
  }

  if (slot === null || slot.id === source.id) return null;
  const target = labels.find((row) => row.id === slot.id);
  if (target === undefined) return null;
  let destParent = effectiveLabelParent(labels, target);
  let slotId = slot.id;
  let after = slot.after;
  if (destParent !== null && effectiveLabelChildren(labels, source.id).length > 0) {
    slotId = destParent;
    after = false;
    destParent = null;
  }
  const siblings = labels
    .filter((row) => row.id !== source.id && effectiveLabelParent(labels, row) === destParent)
    .sort(compareLabelOrder);
  const insertion = siblings.findIndex((row) => row.id === slotId) + (after ? 1 : 0);
  if (effectiveLabelParent(labels, source) === destParent) {
    const current = labels.filter((row) => effectiveLabelParent(labels, row) === destParent).sort(compareLabelOrder);
    const from = current.findIndex((row) => row.id === source.id);
    if (insertion === from) return null;
  }
  const order = neighborLabelOrder(siblings, insertion);
  // A null order refuses the drop: a parent slotted at its own child
  // hoists above itself and misses the sibling list (insertion -1),
  // so there is no order to write.
  if (order === null) return null;
  return { parent: destParent, sortOrder: order };
}
