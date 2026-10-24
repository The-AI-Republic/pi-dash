// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workflow transition gate (ISS-233): the community build allows every
// move — no refusal for any target, no blocked column, plain options — and
// a cloud build refuses exactly what its registered rules deny.
import { afterEach, describe, expect, it, vi } from "vitest";

import type { EditionIssuesProviders, WorkflowRules } from "./providers.js";
import { registerEditionIssuesProviders, resetEditionIssuesProviders } from "./providers.js";
import { WORKFLOW_FLAG } from "./flags.js";
import {
  columnStateId,
  creationBlockedInColumn,
  creationRefusal,
  dropBlockedInColumn,
  isWorkflowGateEnabled,
  refusalMessage,
  stateGateFor,
  stateOptionGate,
  transitionRefusal,
} from "./workflow.js";

const PROJECT = "323e4567-e89b-12d3-a456-426614174002";
const TODO = "623e4567-e89b-12d3-a456-426614174010";
const DONE = "723e4567-e89b-12d3-a456-426614174011";
const BLOCKED = "823e4567-e89b-12d3-a456-426614174012";

/** Rules that close DONE for every move and refuse creation in BLOCKED. */
function closedDoneRules(): WorkflowRules {
  return {
    refusal: ({ toStateId }) => (toStateId === DONE ? { reason: "Finish the review first" } : null),
    creationRefusal: ({ columnKey }) => (columnKey === BLOCKED ? { reason: "Work cannot start here" } : null),
  };
}

function register(providers: EditionIssuesProviders): void {
  registerEditionIssuesProviders(providers);
}

afterEach(() => {
  resetEditionIssuesProviders();
});

describe("community build (flag off)", () => {
  it("is off by default", () => {
    expect(isWorkflowGateEnabled()).toBe(false);
    expect(isWorkflowGateEnabled({})).toBe(false);
  });

  it("allows every transition even when rules are registered", () => {
    register({ workflowRules: closedDoneRules() });
    expect(
      transitionRefusal({ projectId: PROJECT, fromStateId: TODO, toStateId: DONE, purpose: "state-picker" })
    ).toBeNull();
    expect(
      transitionRefusal({ projectId: PROJECT, fromStateId: null, toStateId: BLOCKED, purpose: "create" })
    ).toBeNull();
  });

  it("allows every creation and every drop", () => {
    register({ workflowRules: closedDoneRules() });
    expect(creationRefusal({ projectId: PROJECT, columnKey: BLOCKED })).toBeNull();
    expect(
      creationBlockedInColumn({ projectId: PROJECT, groupDimension: "state", columnKey: BLOCKED })
    ).toBeUndefined();
    expect(
      dropBlockedInColumn({ projectId: PROJECT, groupDimension: "state", columnKey: DONE, fromStateId: TODO })
    ).toBeUndefined();
  });

  it("offers no state gate at all", () => {
    register({ workflowRules: closedDoneRules() });
    expect(stateGateFor({ projectId: PROJECT, fromStateId: TODO, purpose: "state-picker" })).toBeNull();
    expect(stateOptionGate(PROJECT, TODO)).toBeUndefined();
  });
});

describe("cloud build (flag on)", () => {
  const on = { enabled: true };

  it("reads the flag from the record callers pass", () => {
    expect(isWorkflowGateEnabled({ [WORKFLOW_FLAG]: true })).toBe(true);
    expect(isWorkflowGateEnabled({ [WORKFLOW_FLAG]: false })).toBe(false);
  });

  it("refuses only what the rules deny", () => {
    register({ workflowRules: closedDoneRules() });
    expect(
      transitionRefusal({ projectId: PROJECT, fromStateId: TODO, toStateId: DONE, purpose: "state-picker", ...on })
    ).toEqual({
      reason: "Finish the review first",
    });
    expect(
      transitionRefusal({ projectId: PROJECT, fromStateId: TODO, toStateId: BLOCKED, purpose: "state-picker", ...on })
    ).toBeNull();
  });

  it("allows the move when nothing is registered", () => {
    expect(
      transitionRefusal({ projectId: PROJECT, fromStateId: TODO, toStateId: DONE, purpose: "state-picker", ...on })
    ).toBeNull();
    expect(creationRefusal({ projectId: PROJECT, columnKey: BLOCKED, ...on })).toBeNull();
  });

  it("never asks the rules about a move into the item's own state", () => {
    const refusal = vi.fn(() => ({ reason: "never" }));
    register({ workflowRules: { refusal } });
    expect(
      transitionRefusal({ projectId: PROJECT, fromStateId: TODO, toStateId: TODO, purpose: "board-drop", ...on })
    ).toBeNull();
    expect(refusal).not.toHaveBeenCalled();
  });

  it("asks the rules for a create, where there is no current state", () => {
    const refusal = vi.fn(() => null);
    register({ workflowRules: { refusal } });
    transitionRefusal({ projectId: PROJECT, fromStateId: null, toStateId: BLOCKED, purpose: "create", ...on });
    expect(refusal).toHaveBeenCalledWith({
      projectId: PROJECT,
      fromStateId: null,
      toStateId: BLOCKED,
      purpose: "create",
    });
  });

  it("refuses creation only where the rules name a column", () => {
    register({ workflowRules: closedDoneRules() });
    expect(creationRefusal({ projectId: PROJECT, columnKey: BLOCKED, ...on })).toEqual({
      reason: "Work cannot start here",
    });
    expect(creationRefusal({ projectId: PROJECT, columnKey: TODO, ...on })).toBeNull();
  });

  it("treats missing creation rules as open", () => {
    register({ workflowRules: { refusal: () => null } });
    expect(creationRefusal({ projectId: PROJECT, columnKey: BLOCKED, ...on })).toBeNull();
  });
});

describe("columnStateId", () => {
  it("names a state column only when grouping by state", () => {
    expect(columnStateId("state", TODO)).toBe(TODO);
    expect(columnStateId("priority", "urgent")).toBeNull();
    expect(columnStateId(null, TODO)).toBeNull();
    expect(columnStateId("state", "")).toBeNull();
  });
});

describe("column helpers", () => {
  const on = { enabled: true };

  it("passes a state column to the creation rules and reports the reason", () => {
    register({ workflowRules: closedDoneRules() });
    expect(creationBlockedInColumn({ projectId: PROJECT, groupDimension: "state", columnKey: BLOCKED, ...on })).toBe(
      "Work cannot start here"
    );
    expect(
      creationBlockedInColumn({ projectId: PROJECT, groupDimension: "priority", columnKey: BLOCKED, ...on })
    ).toBeUndefined();
  });

  it("falls back to the edition flag when the caller names none", () => {
    register({ workflowRules: closedDoneRules() });
    expect(
      creationBlockedInColumn({ projectId: PROJECT, groupDimension: "state", columnKey: BLOCKED })
    ).toBeUndefined();
  });

  it("refuses a drop into a closed state and reports the reason", () => {
    register({ workflowRules: closedDoneRules() });
    expect(
      dropBlockedInColumn({ projectId: PROJECT, groupDimension: "state", columnKey: DONE, fromStateId: TODO, ...on })
    ).toBe("Finish the review first");
    expect(
      dropBlockedInColumn({ projectId: PROJECT, groupDimension: "state", columnKey: TODO, fromStateId: TODO, ...on })
    ).toBeUndefined();
    expect(
      dropBlockedInColumn({ projectId: PROJECT, groupDimension: "priority", columnKey: DONE, fromStateId: TODO, ...on })
    ).toBeUndefined();
  });

  it("passes the board-drop purpose to the rules", () => {
    const refusal = vi.fn(() => null);
    register({ workflowRules: { refusal } });
    dropBlockedInColumn({ projectId: PROJECT, groupDimension: "state", columnKey: DONE, fromStateId: TODO, ...on });
    expect(refusal).toHaveBeenCalledWith({
      projectId: PROJECT,
      fromStateId: TODO,
      toStateId: DONE,
      purpose: "board-drop",
    });
  });
});

describe("state gates", () => {
  const state = (id: string, name = id) => ({ id, name, group: "started" as const });

  it("marks each refused row disabled with its reason and leaves the rest open", () => {
    register({ workflowRules: closedDoneRules() });
    const gate = stateGateFor({ projectId: PROJECT, fromStateId: TODO, purpose: "state-picker", enabled: true })!;
    expect(gate(state(DONE))).toEqual({ disabled: true, reason: "Finish the review first" });
    expect(gate(state(TODO))).toEqual({ disabled: false });
  });

  it("hands the picker a usable gate only with the flag on", () => {
    register({ workflowRules: closedDoneRules() });
    expect(stateOptionGate(PROJECT, TODO, false)).toBeUndefined();
    const gate = stateOptionGate(PROJECT, TODO, true)!;
    expect(gate(state(DONE))).toEqual({ disabled: true, reason: "Finish the review first" });
  });
});

describe("refusalMessage", () => {
  it("passes a reason through and silences an absent refusal", () => {
    expect(refusalMessage({ reason: "Nope" })).toBe("Nope");
    expect(refusalMessage(null)).toBeUndefined();
    expect(refusalMessage(undefined)).toBeUndefined();
  });
});
