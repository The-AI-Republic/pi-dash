// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Drag resolution and ordering arithmetic for the States tab
// (parity/specs/project-settings-states.md, PSET-031). A drop names the
// row it landed on and the edge it landed against; the move keeps the
// source's group when the target shares it and takes the target's group
// otherwise. The new ordering value is recomputed from the destination
// row's neighbours so the list lands between them without a rewrite of
// the whole group. Pure: the view only measures and commits.

import type { State } from "@pidash/api-client";
import { compareStateSequence, isWorkflowGroup, type WorkflowGroup } from "./groups.js";

/** Distance appended past the tail (and used at an empty edge). */
export const STATE_ORDER_STEP = 65535;

/** Where a drop landed relative to the target row. */
export type DropEdge = "above" | "below";

/** A resolved move: the state, its destination group and ordering value. */
export interface StateMove {
  stateId: string;
  group: WorkflowGroup;
  sequence: number;
}

/**
 * Ordering value for an item inserted at `index` in `rest`, the
 * destination list without the moving row. Between two neighbours the
 * value is their midpoint; at the head it halves the first value; at the
 * tail it steps past the last. An empty list starts from zero.
 */
export function neighborSequence(rest: readonly Pick<State, "sequence">[], index: number): number {
  const previous = rest[index - 1];
  const next = rest[index];
  if (previous !== undefined && next !== undefined) return (previous.sequence + next.sequence) / 2;
  if (next !== undefined) return next.sequence / 2;
  if (previous !== undefined) return previous.sequence + STATE_ORDER_STEP;
  return 0;
}

/**
 * Resolve a drop of `sourceId` onto `targetId` at `edge` within the
 * project's rows. Returns null when the target or source is unknown, or
 * when the drop would leave the row exactly where it already sits — the
 * view then sends nothing.
 */
export function resolveStateMove(
  states: readonly State[],
  sourceId: string,
  targetId: string,
  edge: DropEdge
): StateMove | null {
  const source = states.find((row) => row.id === sourceId);
  const target = states.find((row) => row.id === targetId);
  if (source === undefined || target === undefined || source.id === target.id) return null;
  if (!isWorkflowGroup(target.group)) return null;

  const destination = states
    .filter((row) => row.group === target.group && row.id !== source.id)
    .slice()
    .sort(compareStateSequence);
  // The target shares the destination group and is never the source, so it
  // is always present in the filtered list.
  const targetIndex = destination.findIndex((row) => row.id === target.id);
  const insertion = targetIndex + (edge === "below" ? 1 : 0);

  if (source.group === target.group) {
    const current = states
      .filter((row) => row.group === target.group)
      .slice()
      .sort(compareStateSequence);
    const currentIndex = current.findIndex((row) => row.id === source.id);
    if (currentIndex === insertion) return null;
  }

  return { stateId: source.id, group: target.group, sequence: neighborSequence(destination, insertion) };
}
