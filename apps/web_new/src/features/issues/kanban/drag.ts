// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board drag model (parity/specs/issues.md §5, ISS-037–041). Pure: which
// groupings a card may leave, whether the stored order allows a drop,
// the sort value an insertion takes, the write a cross-cell drop needs,
// and the overlay plus notice a refused drop shows. The board measures
// the pointer and commits the plan; nothing here touches the DOM.

import { ISSUE_PRIORITIES, type IssueListItem, type IssuePatch } from "@pidash/api-client";

import { ORDER_OPTION_LABELS } from "../display/matrix.js";
import { NONE_GROUP_KEY } from "../list/grouping.js";
import type { CardDropEdge } from "./dropTarget.js";

/** Pointer travel before a card press becomes a drag rather than a click. */
export const BOARD_DRAG_THRESHOLD_PX = 4;

/** Fencepost step around the first/last card and into an empty column. */
export const SORT_ORDER_STEP = 65535;

/** Group dimensions a card may leave (spec §5: state, priority, assignees, labels, module, cycle). */
export const DRAGGABLE_DIMENSIONS: readonly string[] = ["state", "priority", "assignees", "labels", "module", "cycle"];

/** Whether drops may change values along one dimension. */
export function isDraggableDimension(dimension: string | null): boolean {
  if (dimension === null) return true;
  return DRAGGABLE_DIMENSIONS.includes(dimension);
}

/** Whether the stored order lets cards move (manual means sort_order). */
export function isManualBoardOrder(orderBy: string): boolean {
  return orderBy === "sort_order";
}

/**
 * Sort value for an insertion at `index` in `rest`, the destination rows
 * without the moving card. Between two neighbours the value is their
 * midpoint; above the first it steps below, below the last it steps
 * above, and an empty column starts at the default step.
 */
export function neighborSortOrder(rest: readonly Pick<IssueListItem, "sort_order">[], index: number): number {
  const previous = rest[index - 1];
  const next = rest[index];
  if (previous !== undefined && next !== undefined) return (previous.sort_order + next.sort_order) / 2;
  if (next !== undefined) return next.sort_order - SORT_ORDER_STEP;
  if (previous !== undefined) return previous.sort_order + SORT_ORDER_STEP;
  return SORT_ORDER_STEP;
}

/** Bridge write a cycle drop needs: add the destination, drop the source. */
export interface CycleMove {
  add: string | null;
  remove: string | null;
}

/** Bridge write a module drop needs. */
export interface ModuleMove {
  add: string[];
  remove: string[];
}

/** Everything one card drop writes. */
export interface BoardMovePlan {
  /** Field changes plus the new sort_order; bridges ride beside it. */
  patch: IssuePatch;
  cycle: CycleMove | null;
  modules: ModuleMove | null;
  /** True when only the position changes inside the same cell. */
  reorderOnly: boolean;
}

export interface BoardMoveInput {
  issue: IssueListItem;
  groupDimension: string | null;
  subDimension: string | null;
  sourceColumn: string;
  sourceLane: string | null;
  destColumn: string;
  destLane: string | null;
  /** Destination rows without the moving card, in display order. */
  destRows: readonly IssueListItem[];
  /** Insertion index into destRows. */
  index: number;
  /** The card's index inside its own cell before the move. */
  sourceIndex: number;
}

function withoutId(ids: readonly string[], id: string): string[] {
  return ids.filter((entry) => entry !== id);
}

function isValidPriority(value: string): value is (typeof ISSUE_PRIORITIES)[number] {
  return (ISSUE_PRIORITIES as readonly string[]).includes(value);
}

interface DimensionMove {
  patch: IssuePatch;
  cycle: CycleMove | null;
  modules: ModuleMove | null;
}

const NO_DIMENSION_MOVE: DimensionMove = { patch: {}, cycle: null, modules: null };

/**
 * Field change for one dimension's source-to-destination hop. Scalar
 * fields are replaced (the empty column clears them); list fields gain
 * the destination and lose the source (the empty column only removes).
 * Cycle and module membership ride their bridges instead of the patch.
 */
export function dimensionMove(
  dimension: string | null,
  sourceKey: string,
  destKey: string,
  issue: IssueListItem
): DimensionMove {
  if (dimension === null || sourceKey === destKey) return NO_DIMENSION_MOVE;
  const sourceNone = sourceKey === NONE_GROUP_KEY;
  const destNone = destKey === NONE_GROUP_KEY;
  switch (dimension) {
    case "state":
      return { patch: { state_id: destNone ? null : destKey }, cycle: null, modules: null };
    case "priority": {
      if (destNone || destKey === "none") return { patch: { priority: "none" }, cycle: null, modules: null };
      if (!isValidPriority(destKey)) return NO_DIMENSION_MOVE;
      return { patch: { priority: destKey }, cycle: null, modules: null };
    }
    case "labels": {
      const withoutSource = sourceNone ? [...issue.label_ids] : withoutId(issue.label_ids, sourceKey);
      const next = destNone || withoutSource.includes(destKey) ? withoutSource : [...withoutSource, destKey];
      return { patch: { label_ids: next }, cycle: null, modules: null };
    }
    case "assignees": {
      const withoutSource = sourceNone ? [...issue.assignee_ids] : withoutId(issue.assignee_ids, sourceKey);
      const next = destNone || withoutSource.includes(destKey) ? withoutSource : [...withoutSource, destKey];
      return { patch: { assignee_ids: next }, cycle: null, modules: null };
    }
    case "cycle": {
      if (destNone) return { patch: {}, cycle: { add: null, remove: sourceKey }, modules: null };
      return { patch: {}, cycle: { add: destKey, remove: null }, modules: null };
    }
    case "module": {
      const remove = sourceNone || !issue.module_ids.includes(sourceKey) ? [] : [sourceKey];
      const add = destNone || issue.module_ids.includes(destKey) ? [] : [destKey];
      return { patch: {}, cycle: null, modules: { add, remove } };
    }
    default:
      return NO_DIMENSION_MOVE;
  }
}

/**
 * The write one drop needs, or null when it changes nothing. A drop back
 * onto its own seat is silent; a cross-cell drop that changes no field
 * (unreachable through the gated board) also sends nothing.
 */
export function planBoardMove(input: BoardMoveInput): BoardMovePlan | null {
  const sameColumn = input.destColumn === input.sourceColumn;
  const sameLane = (input.destLane ?? "") === (input.sourceLane ?? "");
  const group = dimensionMove(input.groupDimension, input.sourceColumn, input.destColumn, input.issue);
  const sub =
    input.subDimension === null
      ? NO_DIMENSION_MOVE
      : dimensionMove(input.subDimension, input.sourceLane ?? "", input.destLane ?? "", input.issue);
  const fieldKeys = Object.keys(group.patch).length + Object.keys(sub.patch).length;
  const bridges =
    (group.cycle === null && sub.cycle === null ? 0 : 1) + (group.modules === null && sub.modules === null ? 0 : 1);
  if (sameColumn && sameLane && input.index === input.sourceIndex) return null;
  if ((!sameColumn || !sameLane) && fieldKeys === 0 && bridges === 0) return null;
  const sort_order = neighborSortOrder(input.destRows, input.index);
  return {
    patch: { ...group.patch, ...sub.patch, sort_order },
    cycle: group.cycle ?? sub.cycle,
    modules: group.modules ?? sub.modules,
    reorderOnly: sameColumn && sameLane,
  };
}

/**
 * Insertion index for a resolved drop: the target card's seat in the
 * destination rows without the moving card (one past it on the below
 * edge), the column end for bare column/cell drops and unknown cards,
 * and the card's own seat while it hovers itself.
 */
export function insertionIndexForDrop(
  destRows: readonly IssueListItem[],
  sourceId: string,
  sourceIndex: number,
  targetIssueId: string | null,
  edge: CardDropEdge | null
): number {
  const without = destRows.filter((row) => row.id !== sourceId);
  if (targetIssueId === null) return without.length;
  if (targetIssueId === sourceId) return sourceIndex;
  const at = without.findIndex((row) => row.id === targetIssueId);
  if (at === -1) return without.length;
  return at + (edge === "below" ? 1 : 0);
}

export interface BoardDropGate {
  orderBy: string;
  groupDimension: string | null;
  subDimension: string | null;
  /** Project-aware edit standing; read-only boards never start a drag. */
  canEdit: boolean;
  /** Destination column key, for the completed-cycle rule. */
  destColumnKey: string;
  /** Cycle ids that still accept members. */
  openCycleIds: ReadonlySet<string>;
  /** Edition workflow rule reason; OSS never supplies one. */
  workflowBlockedReason?: string | undefined;
}

/** Refused drops share one notice title; the reason rides the body. */
export const MOVE_REFUSED_TITLE = "Cannot move the work item";

/**
 * Why a drop onto `destColumnKey` cannot land, or null when it may. The
 * overlay paints over the hovered column while the body explains the
 * refusal toast; a null overlay means the drop refuses silently until
 * release (ungrouped-model groupings).
 */
export function boardDropRefusal(gate: BoardDropGate): { overlay: string | null; toast: string } | null {
  if (!gate.canEdit) {
    return { overlay: null, toast: "You do not have permission to move this work item." };
  }
  if (!isDraggableDimension(gate.groupDimension) || !isDraggableDimension(gate.subDimension)) {
    return { overlay: null, toast: "Cards cannot move while grouped this way." };
  }
  if (!isManualBoardOrder(gate.orderBy)) {
    const label = ORDER_OPTION_LABELS[gate.orderBy] ?? gate.orderBy;
    return {
      overlay: `Ordered by ${label}`,
      toast: `Cards stay put while ordered by ${label}. Switch back to the manual order to move them.`,
    };
  }
  if (
    gate.groupDimension === "cycle" &&
    gate.destColumnKey !== NONE_GROUP_KEY &&
    !gate.openCycleIds.has(gate.destColumnKey)
  ) {
    return { overlay: "Cannot move to a completed cycle", toast: "Cannot move to a completed cycle." };
  }
  if (gate.workflowBlockedReason !== undefined) {
    return { overlay: gate.workflowBlockedReason, toast: gate.workflowBlockedReason };
  }
  return null;
}
