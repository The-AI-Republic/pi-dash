// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module create/edit/delete model (parity/specs/modules.md §2.3, §3.2,
// §7.1–§7.5, MOD-003/009/022–025). Pure: the form shape its defaults,
// prefill and payloads, the title rule, the keyboard sequence the dialog
// pins, the projects a new module may target, and the notice copy each
// outcome shows. Nothing here touches React, the router or the network.

import type { ModuleCreateInput, ModulePatch, ModuleRow, ModuleStatus, ProjectList } from "@pidash/api-client";

import { ROLE_MEMBER } from "../../../core/session/permissions.js";
import type { RangeValue } from "../../issues/index.js";

// ---------------------------------------------------------------- copy

/** Shared outcome titles; the oracle reads these literally. */
export const MODULE_SUCCESS_TITLE = "Success!";
export const MODULE_FAILURE_TITLE = "Error!";
/** Pinned refusal title for a server 403 on delete (§7.5). */
export const MODULE_DELETE_PERMISSION_TITLE = "You don't have permission to perform this action.";

export const MODULE_CREATE_HEADING = "Create module";
export const MODULE_UPDATE_HEADING = "Update module";
export const MODULE_DELETE_HEADING = "Delete module";

export const MODULE_CREATE_SUBMIT = "Create Module";
export const MODULE_UPDATE_SUBMIT = "Update Module";

/** Dialog mode decides the heading, submit label and project picker. */
export type ModuleDialogMode = "create" | "update";

export function moduleDialogHeading(mode: ModuleDialogMode): string {
  return mode === "create" ? MODULE_CREATE_HEADING : MODULE_UPDATE_HEADING;
}

export function moduleSubmitLabel(mode: ModuleDialogMode, pending: boolean): string {
  if (pending) return mode === "create" ? "Creating module…" : "Saving module…";
  return mode === "create" ? MODULE_CREATE_SUBMIT : MODULE_UPDATE_SUBMIT;
}

export function moduleCreateNotice(name: string): string {
  return `“${name}” now appears on the project modules list.`;
}

export function moduleUpdateNotice(): string {
  return "Module updated successfully.";
}

export function moduleDeleteNotice(name: string): string {
  return `“${name}” was deleted.`;
}

/** Fallback sentence when a refusal carries no readable detail. */
export const MODULE_FAILURE_BODY = "Check your connection and try again.";

// ---------------------------------------------------------------- title

/** Longest title the API and the dialog accept (§7.1). */
export const MODULE_TITLE_MAX = 255;

/** Inline title error, or null when the title may be submitted. */
export function moduleTitleError(title: string): string | null {
  if (title.trim() === "") return "Title is required";
  if (title.length > MODULE_TITLE_MAX) return `Title must be ${MODULE_TITLE_MAX} characters or fewer`;
  return null;
}

// ---------------------------------------------------------------- form

/** Everything the dialog edits, in the shape the form holds it. */
export interface ModuleFormState {
  /** Target project; create mode only, because a module never moves. */
  projectId: string;
  title: string;
  description: string;
  range: RangeValue;
  status: ModuleStatus;
  leadId: string | null;
  memberIds: string[];
}

/** A visited, valid visit defaults to Backlog and no people (§7.1). */
export function emptyModuleForm(projectId: string): ModuleFormState {
  return {
    projectId,
    title: "",
    description: "",
    range: { start: null, end: null },
    status: "backlog",
    leadId: null,
    memberIds: [],
  };
}

/** The edit record the dialog prefills from (§7.2). */
export type ModuleEditRecord = Pick<
  ModuleRow,
  "id" | "name" | "description" | "start_date" | "target_date" | "status" | "lead_id" | "member_ids"
>;

export function moduleFormFromRecord(record: ModuleEditRecord, projectId: string): ModuleFormState {
  return {
    projectId,
    title: record.name,
    description: record.description,
    range: { start: record.start_date, end: record.target_date },
    status: record.status,
    leadId: record.lead_id,
    memberIds: [...record.member_ids],
  };
}

/** Create body: every field, dates as explicit nulls when unset (§7.1). */
export function moduleCreatePayload(form: ModuleFormState): ModuleCreateInput {
  return {
    name: form.title.trim(),
    description: form.description,
    start_date: form.range.start,
    target_date: form.range.end,
    status: form.status,
    lead_id: form.leadId,
    member_ids: [...form.memberIds],
  };
}

/** Update body: the same fields through the partial write path (§7.2). */
export function moduleUpdatePayload(form: ModuleFormState): ModulePatch {
  return moduleCreatePayload(form);
}

/** The project a create lands in; create mode's picker may retarget it. */
export function moduleTargetProjectId(form: ModuleFormState): string {
  return form.projectId;
}

/**
 * Whether the open save dialog may close (§7.1): Escape, Cancel and the
 * close control always may; a click outside is swallowed while a write
 * flies, so a slow save cannot throw away the form mid-request.
 */
export function saveDialogDismisses(submitting: boolean, reason: string): boolean {
  return !submitting || reason !== "outside-press";
}

// ---------------------------------------------------------------- tab order

/**
 * The dialog's pinned keyboard sequence (§7.1, bug NEWFRONT-330): date is
 * third, between description and status. The project picker sits in the
 * ambient header and carries no number.
 */
export const MODULE_FORM_TAB_ORDER = {
  title: 1,
  description: 2,
  dateRange: 3,
  status: 4,
  lead: 5,
  members: 6,
  cancel: 7,
  submit: 8,
} as const;

/** The sequence as the parity driver reads it, in visual order. */
export function moduleTabOrderSequence(): number[] {
  return [
    MODULE_FORM_TAB_ORDER.title,
    MODULE_FORM_TAB_ORDER.description,
    MODULE_FORM_TAB_ORDER.dateRange,
    MODULE_FORM_TAB_ORDER.status,
    MODULE_FORM_TAB_ORDER.lead,
    MODULE_FORM_TAB_ORDER.members,
    MODULE_FORM_TAB_ORDER.cancel,
    MODULE_FORM_TAB_ORDER.submit,
  ];
}

// ---------------------------------------------------------------- projects

/**
 * Projects a new module may target (§7.1): modules on, an editor role,
 * not archived. Server order is kept so the picker reads predictably.
 */
export function creatableModuleProjectIds(projects: ProjectList): string[] {
  return projects
    .filter((project) => project.module_view && project.archived_at === null)
    .filter((project) => project.member_role !== null && project.member_role >= ROLE_MEMBER)
    .map((project) => project.id);
}

// ---------------------------------------------------------------- landing

/** Where a landed delete leaves the user (§7.5, bug NEWFRONT-329). */
export interface ModuleDeleteLanding {
  to: string;
  search: Record<string, never>;
  replace: true;
}

/**
 * The plain modules list with every search key dropped, so a delete from
 * the detail page — or from a peek whose `peekModule` sits in the address
 * — returns to the list instead of pointing at a module that is gone.
 * Empty search is deliberate: the list keeps its reading state in the
 * viewer's browser record (§13.3.1), not in the address.
 */
export function moduleDeleteLanding(workspaceSlug: string, projectId: string): ModuleDeleteLanding {
  return { to: `/${workspaceSlug}/projects/${projectId}/modules/`, search: {}, replace: true };
}

// ---------------------------------------------------------------- failure

/** True for the server's permission refusal on a module delete. */
export function isModuleDeletePermissionError(error: unknown): boolean {
  return typeof error === "object" && error !== null && (error as { status?: unknown }).status === 403;
}

/** The server's own sentence for a failed module write, else the fallback. */
export function moduleFailureBody(error: unknown): string {
  if (typeof error === "object" && error !== null) {
    const detail = (error as { detail?: unknown }).detail;
    if (typeof detail === "string" && detail.trim() !== "") return detail.trim();
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim() !== "") return message.trim();
  }
  return MODULE_FAILURE_BODY;
}

/** Confirmation copy that names the module and states the loss (§7.5). */
export function moduleDeleteBody(name: string): string {
  return `“${name}” and all of its data will be permanently removed. This action cannot be undone.`;
}
