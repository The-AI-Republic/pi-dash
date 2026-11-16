// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-draft rules (parity/specs/issues.md §12, ISS-137–141).
// Pure shaping only: how a stored draft becomes a row snapshot the edit
// modal accepts, which fields an inline picker writes, what a copy
// starts from, who may delete a row, and when a due day reads overdue.
// The network stays in api.ts and the page owns the React state.

import type { DraftIssue, DraftIssueCreateInput, ProjectDetails } from "@pidash/api-client";

import { PRIORITY_LEVELS, type PriorityLevel } from "../../../shared/pickers/priorities.js";
import { visibleDescriptionHtml } from "../create/createForm.js";

/**
 * A draft's complete editable snapshot: the shape the modal opens from
 * (create/editForm.ts EditableRow). The stored row already carries every
 * field the form holds, so the copy is a projection, not a rebuild.
 */
export interface DraftSnapshot {
  id: string;
  name: string | null;
  description_html: string;
  state_id: string | null;
  priority: string;
  assignee_ids: string[];
  label_ids: string[];
  start_date: string | null;
  target_date: string | null;
  cycle_id: string | null;
  module_ids: string[];
  estimate_point: string | number | null;
  parent_id: string | null;
}

/** Suffix a copied draft carries before it is saved under a new name. */
export const DRAFT_COPY_SUFFIX = " (copy)";

/** The row the edit modal opens from. Defensive copies keep the query data intact. */
export function draftSnapshot(draft: DraftIssue): DraftSnapshot {
  return {
    id: draft.id,
    name: draft.name,
    description_html: draft.description_html,
    state_id: draft.state_id,
    priority: draft.priority,
    assignee_ids: [...draft.assignee_ids],
    label_ids: [...draft.label_ids],
    start_date: draft.start_date ?? null,
    target_date: draft.target_date ?? null,
    cycle_id: draft.cycle_id,
    module_ids: [...draft.module_ids],
    estimate_point: draft.estimate_point ?? null,
    parent_id: draft.parent_id,
  };
}

/**
 * The state a draft's picker reads: its own pick, or the project's
 * default while it holds none. The old block draws the default for an
 * unset draft (its state dropdown falls back to the default), so a
 * freshly created draft reads "Todo" rather than an empty trigger
 * (ISS-138 oracle).
 */
export function draftStateId(
  draft: Pick<DraftIssue, "state_id">,
  project: Pick<ProjectDetails, "default_state"> | undefined
): string | null {
  return draft.state_id ?? project?.default_state ?? null;
}

/** The duplicated payload a copy starts from: same values, name suffixed. */
export function copyDraftInput(draft: DraftIssue): DraftIssueCreateInput {
  const input: DraftIssueCreateInput = {
    name: `${draft.name ?? ""}${DRAFT_COPY_SUFFIX}`.trim(),
  };
  // A body without content stays off the payload, like every other
  // unset field; an empty description on a copy would otherwise replace
  // the placeholder the saved copy opens with.
  const description = visibleDescriptionHtml(draft.description_html);
  if (description !== "") input.description_html = description;
  if (draft.project_id !== null) input.project_id = draft.project_id;
  if (draft.state_id !== null) input.state_id = draft.state_id;
  if (draft.priority !== "none") input.priority = draft.priority;
  if (draft.assignee_ids.length > 0) input.assignee_ids = [...draft.assignee_ids];
  if (draft.label_ids.length > 0) input.label_ids = [...draft.label_ids];
  if (draft.start_date !== undefined && draft.start_date !== null) input.start_date = draft.start_date;
  if (draft.target_date !== undefined && draft.target_date !== null) input.target_date = draft.target_date;
  if (draft.cycle_id !== null) input.cycle_id = draft.cycle_id;
  if (draft.module_ids.length > 0) input.module_ids = [...draft.module_ids];
  if (draft.estimate_point !== undefined && draft.estimate_point !== null)
    input.estimate_point = String(draft.estimate_point);
  if (draft.parent_id !== null) input.parent_id = draft.parent_id;
  return input;
}

/** The title a copy opens with, as the duplicate payload stores it. */
export function copyDraftTitle(draft: DraftIssue): string {
  return `${draft.name ?? ""}${DRAFT_COPY_SUFFIX}`;
}

/** Whether a priority string is one the picker offers. */
export function asPriority(value: string): PriorityLevel {
  return (PRIORITY_LEVELS as readonly string[]).includes(value) ? (value as PriorityLevel) : "none";
}

/** The workspace role facts that decide a draft row's actions (DRAFT-025). */
export interface DraftViewer {
  /** The signed-in user's id, or null while the session read is unresolved. */
  meId: string | null;
  /** Workspace admin, not merely a project admin. */
  isAdmin: boolean;
  /** Workspace member or admin. Guests are workspace members but cannot write. */
  canWrite: boolean;
}

/** What the current viewer may do with one draft row. */
export interface DraftAbilities {
  /** Edit, fetch and delete: the draft's creator or a workspace admin. */
  manage: boolean;
  /** Make a copy: a member/admin acting on their own draft. */
  copy: boolean;
  /** Promote to a work item: a member/admin acting on their own draft. */
  convert: boolean;
}

/**
 * The delete standing (drafts spec §4.6, DRAFT-011): the draft's
 * creator at any role, or a workspace admin. A project-only admin who
 * is not a workspace admin does not qualify, and project standing is
 * otherwise irrelevant. The server enforces the same rule.
 */
export function canDeleteDraft(
  draft: Pick<DraftIssue, "created_by">,
  viewer: { meId: string | null; isWorkspaceAdmin: boolean }
): boolean {
  if (viewer.meId !== null && draft.created_by === viewer.meId) return true;
  return viewer.isWorkspaceAdmin;
}

/**
 * The DRAFT-025 matrix for one row. Ownership is checked by user id, and
 * the admin half is the workspace role, never the project role: a project
 * admin who is only a workspace member must not manage someone else's
 * draft. Guests own their drafts and may manage them, but cannot create
 * copies or convert them. Management reuses the delete standing above,
 * since creator-or-workspace-admin governs edit, fetch and delete alike.
 */
export function draftAbilities(draft: Pick<DraftIssue, "created_by">, viewer: DraftViewer): DraftAbilities {
  const owns = viewer.meId !== null && draft.created_by === viewer.meId;
  return {
    manage: canDeleteDraft(draft, { meId: viewer.meId, isWorkspaceAdmin: viewer.isAdmin }),
    copy: owns && viewer.canWrite,
    convert: owns && viewer.canWrite,
  };
}

/** State groups that read as finished for the overdue flag. */
const CLOSED_STATE_GROUPS: ReadonlySet<string> = new Set(["completed", "cancelled"]);

/**
 * Whether a due day reads as overdue (ISS-138): strictly before today,
 * and never on finished work. A draft whose state has not resolved yet
 * (no group) reads plain rather than red.
 */
export function isDraftOverdue(
  targetDate: string | null,
  stateGroup: string | null | undefined,
  today: string
): boolean {
  if (targetDate === null || stateGroup === null || stateGroup === undefined) return false;
  if (CLOSED_STATE_GROUPS.has(stateGroup)) return false;
  return targetDate.slice(0, 10) < today;
}

/**
 * One inline picker write: the field plus the value that goes on the
 * draft PATCH (ISS-138). Clearing a date or a cycle stores null; the
 * module list travels whole because the server replaces the set.
 */
export type DraftFieldPatch = DraftIssueCreateInput;

/** Patch for a state pick: drafts always hold a state once picked. */
export function statePatch(stateId: string): DraftFieldPatch {
  return { state_id: stateId };
}

export function priorityPatch(priority: PriorityLevel): DraftFieldPatch {
  return { priority };
}

export function labelPatch(labelIds: readonly string[]): DraftFieldPatch {
  return { label_ids: [...labelIds] };
}

export function assigneePatch(assigneeIds: readonly string[]): DraftFieldPatch {
  return { assignee_ids: [...assigneeIds] };
}

/**
 * A start-date pick writes both ends of the range: the server compares
 * the two only when both arrive in one request, so a lone start write
 * would bypass the order check (parity/specs/drafts.md §8.4). A missing
 * counterpart travels as null, which clears it.
 */
export function startDatePatch(date: string | null, targetDate: string | null | undefined): DraftFieldPatch {
  return { start_date: date, target_date: targetDate ?? null };
}

/** A due-date pick writes both ends, mirroring the start pick above. */
export function targetDatePatch(date: string | null, startDate: string | null | undefined): DraftFieldPatch {
  return { start_date: startDate ?? null, target_date: date };
}

/**
 * Whether a start/due pair keeps the range order: a missing end is
 * always fine, otherwise the start day must not fall after the due day
 * (parity/specs/drafts.md §3.2). Only the day part is compared.
 */
export function isDateRangeValid(startDate: string | null, targetDate: string | null): boolean {
  if (startDate === null || targetDate === null) return true;
  return startDate.slice(0, 10) <= targetDate.slice(0, 10);
}

/**
 * The labels trigger text: the empty prompt with no selection, the
 * chosen names once every one resolves, otherwise the selected count
 * while the label read is still pending (parity/specs/drafts.md §3.2).
 */
export function draftLabelsTriggerText(
  labelIds: readonly string[],
  labels: readonly { id: string; name: string }[],
  emptyText: string
): string {
  if (labelIds.length === 0) return emptyText;
  const names = labelIds.map((id) => labels.find((label) => label.id === id)?.name);
  if (names.every((name) => name !== undefined)) return names.join(", ");
  return `${labelIds.length} selected`;
}

export function cyclePatch(cycleId: string | null): DraftFieldPatch {
  return { cycle_id: cycleId };
}

export function modulePatch(moduleIds: readonly string[]): DraftFieldPatch {
  return { module_ids: [...moduleIds] };
}

export function estimatePatch(estimatePoint: string | null): DraftFieldPatch {
  return { estimate_point: estimatePoint };
}
