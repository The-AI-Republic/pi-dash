// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workflow-state grouping for the States settings tab
// (parity/specs/project-settings-states.md, PSET-028). Seven groups exist
// and nothing may add, rename or delete one; a group with no rows still
// renders as an empty card. Rows order by their sequence value inside a
// group and groups by the fixed list below. Pure: no React, router or
// network.

import type { State } from "@pidash/api-client";

/**
 * The seven workflow groups in display order. Triage is deliberately
 * absent: it is not a settings group and the server refuses creation
 * into it.
 */
export const WORKFLOW_GROUPS = ["backlog", "unstarted", "started", "review", "test", "completed", "cancelled"] as const;

export type WorkflowGroup = (typeof WORKFLOW_GROUPS)[number];

/** Human label for a group card header. */
export const WORKFLOW_GROUP_LABELS: Record<WorkflowGroup, string> = {
  backlog: "Backlog",
  unstarted: "Unstarted",
  started: "Started",
  review: "Review",
  test: "Test",
  completed: "Completed",
  cancelled: "Cancelled",
};

/**
 * Swatch a fresh create form starts on, per group. The four end groups
 * borrow the palette the state pickers use for their glyphs so a new
 * row reads as part of its group before anything is chosen.
 */
export const WORKFLOW_GROUP_SWATCHES: Record<WorkflowGroup, string> = {
  backlog: "#9CA3AF",
  unstarted: "#6B7280",
  started: "#F59E0B",
  review: "#3B82F6",
  test: "#8B5CF6",
  completed: "#10B981",
  cancelled: "#EF4444",
};

/** Groups whose card starts collapsed; every other card starts open. */
export const COLLAPSED_ON_LOAD: readonly WorkflowGroup[] = ["review"];

/** One rendered group card: its key, label and ordered rows. */
export interface GroupedStates {
  group: WorkflowGroup;
  label: string;
  rows: State[];
}

/** Ascending sequence within a group; equal values keep server order. */
export function compareStateSequence(a: Pick<State, "sequence">, b: Pick<State, "sequence">): number {
  return a.sequence - b.sequence;
}

/**
 * Bucket rows into all seven groups, dropping anything outside the fixed
 * set (a stray triage row never reaches this tab: the server excludes it,
 * and a future group key is not ours to render). Unknown keys are ignored
 * rather than invented into a card.
 */
export function groupStates(states: readonly State[]): GroupedStates[] {
  const buckets: Record<WorkflowGroup, State[]> = {
    backlog: [],
    unstarted: [],
    started: [],
    review: [],
    test: [],
    completed: [],
    cancelled: [],
  };
  for (const state of states) {
    if (!isWorkflowGroup(state.group)) continue;
    buckets[state.group].push(state);
  }
  return WORKFLOW_GROUPS.map((group) => ({
    group,
    label: WORKFLOW_GROUP_LABELS[group],
    rows: buckets[group].slice().sort(compareStateSequence),
  }));
}

/** True for one of the seven settings groups; false for triage or unknown. */
export function isWorkflowGroup(value: string): value is WorkflowGroup {
  return (WORKFLOW_GROUPS as readonly string[]).includes(value);
}

/**
 * True when a group's rows can be dragged (PSET-031): editing is on and
 * the group holds more than one state. A lone state has nowhere to move
 * within its group, so the grip is withheld.
 */
export function canDragGroupRows(rowCount: number, editable: boolean): boolean {
  return editable && rowCount > 1;
}

/**
 * Share of the group a row occupies, 1-based over the group size, used
 * by the row's share bar. A row outside a group (size 0) reads 0.
 */
export function rowShare(position: number, groupSize: number): number {
  if (groupSize <= 0) return 0;
  return position / groupSize;
}
