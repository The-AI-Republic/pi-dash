// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workflow transition gate (parity/specs/issues.md §20/§23, ISS-233). In
// the community build every transition goes through: the gate answers no
// refusal for any target and every state option stays enabled, which is
// the "no rules" behavior the oracle pins. A cloud build registers
// transition rules through the provider seam and turns the flag on; the
// gate then refuses the moves those rules deny and names the project's
// blocking state in the message.

import { edition } from "../../../core/edition/index.js";
import { isEditionFlagOn, WORKFLOW_FLAG } from "./flags.js";
import type { GatedState, TransitionPurpose, WorkflowRefusal } from "./providers.js";
import { getEditionIssuesProviders } from "./providers.js";

/** True when this build enforces workflow transitions. */
export function isWorkflowGateEnabled(flags: Record<string, boolean> = edition.flags): boolean {
  return isEditionFlagOn(WORKFLOW_FLAG, flags);
}

/**
 * The refusal for a state change, or null when it may proceed. Every
 * surface (state pickers, board drops, creation) asks this one function,
 * so a build either enforces the configuration everywhere or nowhere.
 */
export function transitionRefusal(input: {
  projectId: string;
  fromStateId: string | null;
  toStateId: string;
  purpose: TransitionPurpose;
  enabled?: boolean | undefined;
}): WorkflowRefusal | null {
  const enabled = input.enabled ?? isWorkflowGateEnabled();
  if (!enabled) return null;
  const rules = getEditionIssuesProviders().workflowRules;
  if (rules === undefined) return null;
  if (input.fromStateId !== null && input.fromStateId === input.toStateId) return null;
  return rules.refusal({
    projectId: input.projectId,
    fromStateId: input.fromStateId,
    toStateId: input.toStateId,
    purpose: input.purpose,
  });
}

/** The refusal for creating inside a board column, or null. */
export function creationRefusal(input: {
  projectId: string;
  columnKey: string;
  enabled?: boolean | undefined;
}): WorkflowRefusal | null {
  const enabled = input.enabled ?? isWorkflowGateEnabled();
  if (!enabled) return null;
  const rules = getEditionIssuesProviders().workflowRules;
  if (rules === undefined || rules.creationRefusal === undefined) return null;
  return rules.creationRefusal({ projectId: input.projectId, columnKey: input.columnKey });
}

/** The message a refused option or drop announces. */
export function refusalMessage(refusal: WorkflowRefusal | null | undefined): string | undefined {
  return refusal === null || refusal === undefined ? undefined : refusal.reason;
}

/** The display dimension whose column keys are state ids. */
const STATE_DIMENSION = "state";

/** The state a board/list column stands for, when it groups by state. */
export function columnStateId(groupDimension: string | null, columnKey: string): string | null {
  return groupDimension === STATE_DIMENSION && columnKey !== "" ? columnKey : null;
}

/**
 * Why creation inside one column is refused, or undefined when it is
 * allowed (or the column does not stand for a state). Every surface that
 * offers creation asks this, so the gate covers quick-adds, group headers
 * and the create dialog's prefill alike.
 */
export function creationBlockedInColumn(input: {
  projectId: string;
  groupDimension: string | null;
  columnKey: string;
  enabled?: boolean | undefined;
}): string | undefined {
  const stateId = columnStateId(input.groupDimension, input.columnKey);
  if (stateId === null) return undefined;
  return refusalMessage(
    creationRefusal({
      projectId: input.projectId,
      columnKey: stateId,
      ...(input.enabled === undefined ? {} : { enabled: input.enabled }),
    })
  );
}

/**
 * Why a card may not move from its current state into one column, or
 * undefined when the move is open to it. A card already in the target
 * column never reports a refusal: dropping it back is a position change.
 */
export function dropBlockedInColumn(input: {
  projectId: string;
  groupDimension: string | null;
  columnKey: string;
  fromStateId: string | null;
  enabled?: boolean | undefined;
}): string | undefined {
  const toStateId = columnStateId(input.groupDimension, input.columnKey);
  if (toStateId === null) return undefined;
  return refusalMessage(
    transitionRefusal({
      projectId: input.projectId,
      fromStateId: input.fromStateId,
      toStateId,
      purpose: "board-drop",
      ...(input.enabled === undefined ? {} : { enabled: input.enabled }),
    })
  );
}

/** One state row's gate: enabled, or disabled with the rule's sentence. */
export interface StateGateEntry {
  disabled: boolean;
  reason?: string;
}

/** A per-state filter a picker applies before listing its rows. */
export type StateGate = (state: GatedState) => StateGateEntry;

/**
 * The gate a detail or sub-item state picker hands its rows, or undefined
 * when this build enforces no rules. The picker then keeps every row as it
 * was, so the community build draws no disabled option (ISS-233).
 */
export function stateOptionGate(
  projectId: string,
  fromStateId: string | null,
  enabled?: boolean | undefined
): StateGate | undefined {
  return (
    stateGateFor({ projectId, fromStateId, purpose: "state-picker", ...(enabled === undefined ? {} : { enabled }) }) ??
    undefined
  );
}

/**
 * The gate a state picker applies: with the workflow off every row is
 * open; with it on each row carries its own refusal so the option stays
 * visible but greyed, naming the rule. The identity gate is returned
 * unchanged so a picker can skip the whole mechanism in the community
 * build.
 */
export function stateGateFor(input: {
  projectId: string;
  fromStateId: string | null;
  purpose: TransitionPurpose;
  enabled?: boolean | undefined;
}): StateGate | null {
  const enabled = input.enabled ?? isWorkflowGateEnabled();
  if (!enabled) return null;
  return (state) => {
    const refusal = transitionRefusal({
      projectId: input.projectId,
      fromStateId: input.fromStateId,
      toStateId: state.id,
      purpose: input.purpose,
      enabled,
    });
    return refusal === null ? { disabled: false } : { disabled: true, reason: refusal.reason };
  };
}
