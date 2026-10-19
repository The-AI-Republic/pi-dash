// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Work-item edit rules for the modal (parity/specs/issues.md §11,
// ISS-126): how an existing row becomes the modal's starting form, which
// fields an edit actually sends, and how cycle and module memberships are
// reconciled against what the modal opened with. The create rules live in
// createForm.ts; this module only shapes an edit.

import type { DraftIssue, DraftIssueCreateInput, Issue, IssuePatch } from "@pidash/api-client";

import { executionTargetPatch, type ExecutionSelection } from "../../../shared/pickers/index.js";
import { NO_ESTIMATE_ID } from "../../../shared/pickers/estimates.js";
import { normalizeTitle, visibleDescriptionHtml, type CreateFormState } from "./createForm.js";

/** The row shape an edit opens from: live issues and drafts share it. */
export interface EditableRow {
  id: string;
  name: string | null;
  description_html?: string | undefined;
  state_id: string | null;
  priority: string;
  assignee_ids: string[];
  label_ids: string[];
  start_date?: string | null | undefined;
  target_date?: string | null | undefined;
  cycle_id?: string | null | undefined;
  module_ids?: string[] | undefined;
  estimate_point?: string | number | null | undefined;
  parent_id: string | null;
}

/** What the modal opened with, kept for diffing on submit. */
export interface EditBaseline {
  /** The row's project, fixed while editing. */
  projectId: string;
  /** Fields as the server answered them, normalized like the form. */
  form: EditFormFields;
}

/** The comparable half of the form (identity lives outside it). */
export type EditFormFields = Omit<CreateFormState, "projectId">;

/** Execution targets compare by their two written columns. */
function sameExecution(left: ExecutionSelection | null, right: ExecutionSelection | null): boolean {
  if (left === null || right === null) return left === right;
  const a = executionTargetPatch(left);
  const b = executionTargetPatch(right);
  return a.assigned_pod_id === b.assigned_pod_id && a.agent_executor === b.agent_executor;
}

/** Assignee and label lists compare as sets: order is not an edit. */
function sameSet(left: readonly string[], right: readonly string[]): boolean {
  if (left.length !== right.length) return false;
  const rightSet = new Set(right);
  return left.every((entry) => rightSet.has(entry));
}

function dateOf(value: string | null | undefined): string | null {
  return value === undefined || value === "" ? null : value;
}

/**
 * The estimate picker's value: the selected point id, or the clear row
 * when the row holds none. Issue detail reads the join id; list rows
 * carry the point's value instead, so a raw number cannot round-trip.
 */
function estimateOf(value: string | number | null | undefined): string | null {
  if (value === undefined || value === null || value === "") return null;
  return String(value) === NO_ESTIMATE_ID ? null : String(value);
}

/** Turn a live issue or draft into the modal's starting form. */
export function formFromRow(row: EditableRow, projectId: string | null): CreateFormState {
  return {
    projectId,
    name: row.name ?? "",
    descriptionHtml: row.description_html ?? "",
    stateId: row.state_id,
    priority: row.priority,
    assigneeIds: [...row.assignee_ids],
    labelIds: [...row.label_ids],
    startDate: dateOf(row.start_date),
    targetDate: dateOf(row.target_date),
    cycleId: dateOf(row.cycle_id),
    moduleIds: [...(row.module_ids ?? [])],
    estimatePoint: estimateOf(row.estimate_point),
    execution: null,
    parentId: row.parent_id,
    gitWorkBranch: "",
  };
}

/**
 * Cycle and module membership the edit still owes after its PATCH
 * (ISS-124/ISS-126): the cycle leaves its old bridge when cleared or
 * swapped, and modules are diffed by id. Memberships equal to the
 * baseline owe nothing, so an unrelated edit never re-points a bridge.
 */
export interface EditMembership {
  /** Cycle to detach from, when the form moved away from one. */
  removedCycleId: string | null;
  /** Cycle to attach to, when the form names a different one. */
  addedCycleId: string | null;
  /** Module ids to attach. */
  addedModuleIds: string[];
  /** Module ids to detach. */
  removedModuleIds: string[];
}

export function editMembership(form: CreateFormState, baseline: EditBaseline): EditMembership {
  const before = baseline.form;
  const cycleChanged = form.cycleId !== before.cycleId;
  return {
    removedCycleId: cycleChanged && before.cycleId !== null ? before.cycleId : null,
    addedCycleId: cycleChanged ? form.cycleId : null,
    addedModuleIds: form.moduleIds.filter((id) => !before.moduleIds.includes(id)),
    removedModuleIds: before.moduleIds.filter((id) => !form.moduleIds.includes(id)),
  };
}

export function hasEditMembership(membership: EditMembership): boolean {
  return (
    membership.removedCycleId !== null ||
    membership.addedCycleId !== null ||
    membership.addedModuleIds.length > 0 ||
    membership.removedModuleIds.length > 0
  );
}

/** A live row narrowed to the fields an edit needs (keeps types honest). */
export function editableFromIssue(issue: Issue): EditableRow {
  return issue;
}

/** A draft narrowed to the same fields. */
export function editableFromDraft(draft: DraftIssue): EditableRow {
  return draft;
}

/** The baseline an edit compares against, derived from the same form. */
export function baselineFor(form: CreateFormState): EditBaseline {
  if (form.projectId === null) throw new Error("An edit needs a project");
  return {
    projectId: form.projectId,
    form: {
      name: normalizeTitle(form.name),
      descriptionHtml: visibleDescriptionHtml(form.descriptionHtml),
      stateId: form.stateId,
      priority: form.priority,
      assigneeIds: [...form.assigneeIds],
      labelIds: [...form.labelIds],
      startDate: form.startDate,
      targetDate: form.targetDate,
      cycleId: form.cycleId,
      moduleIds: [...form.moduleIds],
      estimatePoint: form.estimatePoint,
      parentId: form.parentId,
      gitWorkBranch: form.gitWorkBranch.trim(),
      execution: form.execution,
    },
  };
}

/**
 * The PATCH an edit sends (ISS-126): only fields whose value changed.
 * Identity and project never ride along — the caller already knows the
 * address — and the description travels whenever its text changed,
 * including when it was cleared.
 */
export function buildEditPayload(form: CreateFormState, baseline: EditBaseline): IssuePatch {
  const patch: IssuePatch = {};
  const before = baseline.form;
  const description = visibleDescriptionHtml(form.descriptionHtml);
  if (description !== before.descriptionHtml) patch.description_html = description;

  const name = normalizeTitle(form.name);
  if (name !== before.name) patch.name = name;
  if (form.stateId !== before.stateId) patch.state_id = form.stateId;
  if (form.priority !== before.priority) {
    patch.priority = form.priority as NonNullable<IssuePatch["priority"]>;
  }
  if (!sameSet(form.assigneeIds, before.assigneeIds)) patch.assignee_ids = [...form.assigneeIds];
  if (!sameSet(form.labelIds, before.labelIds)) patch.label_ids = [...form.labelIds];
  if (form.startDate !== before.startDate) patch.start_date = form.startDate;
  if (form.targetDate !== before.targetDate) patch.target_date = form.targetDate;
  if (form.estimatePoint !== before.estimatePoint) patch.estimate_point = form.estimatePoint;
  if (form.parentId !== before.parentId) patch.parent_id = form.parentId;
  const branch = form.gitWorkBranch.trim();
  if (branch !== "" && branch !== before.gitWorkBranch) patch.git_work_branch = branch;
  // The execution target is written through the same patch: the pod pin
  // and the executor override travel as a pair (ISS-218).
  if (form.execution === null) {
    if (before.execution !== null) {
      patch.assigned_pod_id = null;
      patch.agent_executor = null;
    }
  } else if (!sameExecution(form.execution, before.execution)) {
    const target = executionTargetPatch(form.execution);
    patch.assigned_pod_id = target.assigned_pod_id;
    patch.agent_executor = target.agent_executor;
  }
  return patch;
}

/** Whether an edit has anything to send. */
export function isPatchEmpty(patch: IssuePatch): boolean {
  return Object.keys(patch).length === 0;
}

/**
 * The form for a JSON snapshot of a row: the dialog's state persists as
 * an edit is reopened, so the snapshot is normalized on the way back in.
 * Only fields the form holds ride along; unknown keys are dropped.
 */
export function formFromSnapshot(snapshot: unknown, projectId: string | null): CreateFormState | null {
  const row = parseSnapshot(snapshot);
  return row === null ? null : formFromRow(row, projectId);
}

function parseSnapshot(snapshot: unknown): EditableRow | null {
  if (typeof snapshot !== "object" || snapshot === null) return null;
  const raw = snapshot as Record<string, unknown>;
  if (typeof raw["id"] !== "string") return null;
  const strings = (value: unknown): string[] =>
    Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === "string") : [];
  return {
    id: raw["id"],
    name: typeof raw["name"] === "string" ? raw["name"] : null,
    description_html: typeof raw["description_html"] === "string" ? raw["description_html"] : undefined,
    state_id: typeof raw["state_id"] === "string" ? raw["state_id"] : null,
    priority: typeof raw["priority"] === "string" ? raw["priority"] : "none",
    assignee_ids: strings(raw["assignee_ids"]),
    label_ids: strings(raw["label_ids"]),
    start_date: typeof raw["start_date"] === "string" ? raw["start_date"] : null,
    target_date: typeof raw["target_date"] === "string" ? raw["target_date"] : null,
    cycle_id: typeof raw["cycle_id"] === "string" ? raw["cycle_id"] : null,
    module_ids: strings(raw["module_ids"]),
    estimate_point:
      typeof raw["estimate_point"] === "string" || typeof raw["estimate_point"] === "number"
        ? raw["estimate_point"]
        : null,
    parent_id: typeof raw["parent_id"] === "string" ? raw["parent_id"] : null,
  };
}

/** The payload a publish sends: the form's stored values (ISS-130). */
export function buildPublishInput(form: CreateFormState): DraftIssueCreateInput {
  const input: DraftIssueCreateInput = { name: normalizeTitle(form.name) };
  const description = visibleDescriptionHtml(form.descriptionHtml);
  if (description !== "") input.description_html = description;
  if (form.projectId !== null) input.project_id = form.projectId;
  if (form.stateId !== null) input.state_id = form.stateId;
  if (form.priority !== "none") input.priority = form.priority as NonNullable<DraftIssueCreateInput["priority"]>;
  if (form.assigneeIds.length > 0) input.assignee_ids = [...form.assigneeIds];
  if (form.labelIds.length > 0) input.label_ids = [...form.labelIds];
  if (form.startDate !== null) input.start_date = form.startDate;
  if (form.targetDate !== null) input.target_date = form.targetDate;
  if (form.parentId !== null) input.parent_id = form.parentId;
  return input;
}
