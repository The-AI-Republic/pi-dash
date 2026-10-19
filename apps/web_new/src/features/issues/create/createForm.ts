// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Create-dialog form rules (parity/specs/issues.md §11, ISS-117–123).
// Pure state shaping only: the dialog component owns focus, the network
// calls stay in api/, and these helpers decide what a create submits,
// which projects may receive it, when a title is acceptable, what the
// discard guard still counts as work, and how the form looks after a
// create-with-more reset.

import type { DraftIssueCreateInput, IssueCreateInput } from "@pidash/api-client";

import { isStoredHtmlEmpty, storedTextContent } from "../../../shared/editor/index.js";
import { executionTargetPatch, type ExecutionSelection } from "../../../shared/pickers/index.js";
import { TITLE_MAX_LENGTH, type CreateProjectOption } from "./createProjects.js";

export { TITLE_MAX_LENGTH } from "./createProjects.js";
export type { CreateProjectLike, CreateProjectOption } from "./createProjects.js";
export { selectCreatableProjects, selectCreatableWorkItemProjects, toCreateProjectOption } from "./createProjects.js";

/** How long the assistant title helper may run before the dialog gives up. */
export const TITLE_GENERATION_TIMEOUT_MS = 30_000;
/** Draft title when the guard saves a form that never got one. */
export const UNTITLED_DRAFT_NAME = "Untitled";

/** Everything the create dialog holds while it is open. */
export interface CreateFormState {
  projectId: string | null;
  name: string;
  descriptionHtml: string;
  stateId: string | null;
  priority: string;
  assigneeIds: string[];
  labelIds: string[];
  startDate: string | null;
  targetDate: string | null;
  cycleId: string | null;
  moduleIds: string[];
  estimatePoint: string | null;
  execution: ExecutionSelection | null;
  parentId: string | null;
  /** Optional remote branch the item pins to (ISS-128). */
  gitWorkBranch: string;
}

/** A blank form. `descriptionHtml` seeds the editor's default content. */
export function emptyCreateForm(projectId: string | null, descriptionHtml = ""): CreateFormState {
  return {
    projectId,
    name: "",
    descriptionHtml,
    stateId: null,
    priority: "none",
    assigneeIds: [],
    labelIds: [],
    startDate: null,
    targetDate: null,
    cycleId: null,
    moduleIds: [],
    estimatePoint: null,
    execution: null,
    parentId: null,
    gitWorkBranch: "",
  };
}

/** The form after a successful create while Create more is on (ISS-119). */
export function resetForCreateMore(form: CreateFormState, descriptionDefault = ""): CreateFormState {
  return emptyCreateForm(form.projectId, descriptionDefault);
}

/** Whether the dialog may file under a project id (ISS-123). */
export function isCreatableProjectId(projectId: string, available: readonly CreateProjectOption[]): boolean {
  return available.some((project) => project.id === projectId);
}

/** Keep a preferred project when it is still offered, else the first one. */
export function resolveProjectId(projects: readonly CreateProjectOption[], preferred: string | null): string | null {
  if (preferred !== null && projects.some((project) => project.id === preferred)) return preferred;
  return projects[0]?.id ?? null;
}

/**
 * The project the form files under. The dialog only renders the form
 * once at least one creatable project exists, so the first offered row
 * backs up a null form value; an empty string keeps the pickers typed
 * for a caller that ignores that gate.
 */
export function targetProjectId(projectId: string | null, available: readonly CreateProjectOption[]): string {
  return projectId ?? available[0]?.id ?? "";
}

/** Longest git branch name the API accepts (ISS-128). */
export const GIT_BRANCH_MAX_LENGTH = 128;
/** Characters a branch name may carry; empty means "create a new branch". */
const GIT_BRANCH_PATTERN = /^[A-Za-z0-9._/-]*$/;

/**
 * Inline message for the advanced branch field, or null when it may be
 * sent (ISS-128). Empty is valid: the run creates a branch of its own.
 */
export function gitBranchError(branch: string): string | null {
  if (branch.length > GIT_BRANCH_MAX_LENGTH)
    return `Keep the branch name to ${GIT_BRANCH_MAX_LENGTH} characters or fewer.`;
  if (!GIT_BRANCH_PATTERN.test(branch)) return "Use only letters, numbers, '.', '_', '/' and '-'.";
  return null;
}

/** Trimmed title, or empty when the field holds only whitespace. */
export function normalizeTitle(name: string): string {
  return name.trim();
}

/** True when the title may be submitted as typed (ISS-118). */
export function isTitleAcceptable(name: string): boolean {
  const trimmed = normalizeTitle(name);
  return trimmed.length > 0 && trimmed.length <= TITLE_MAX_LENGTH;
}

/** Inline title message, or null when the field is fine. */
export function titleError(name: string): string | null {
  if (normalizeTitle(name) === "") return "Give the work item a title.";
  if (name.length > TITLE_MAX_LENGTH) return `Keep the title to ${TITLE_MAX_LENGTH} characters or fewer.`;
  return null;
}

/** Plain text of the description without surrounding whitespace. */
export function descriptionText(descriptionHtml: string): string {
  return storedTextContent(descriptionHtml).trim();
}

/**
 * The stored description, or an empty string when it holds no visible
 * content. The editor emits an empty paragraph on mount, which is not
 * something the user typed and must not count as work or travel in the
 * create payload.
 */
export function visibleDescriptionHtml(descriptionHtml: string): string {
  return isStoredHtmlEmpty(descriptionHtml) ? "" : descriptionHtml;
}

/** Whether the assistant may name this work item from its description (ISS-118). */
export function canGenerateTitle(input: {
  name: string;
  descriptionHtml: string;
  assistantConfigured: boolean;
}): boolean {
  return (
    input.assistantConfigured && normalizeTitle(input.name) === "" && descriptionText(input.descriptionHtml) !== ""
  );
}

/** The create payload: always the title, plus every field the user set (ISS-117). */
export function buildCreatePayload(form: CreateFormState): IssueCreateInput {
  const payload: IssueCreateInput = { name: normalizeTitle(form.name) };
  const description = visibleDescriptionHtml(form.descriptionHtml);
  if (description !== "") payload.description_html = description;
  if (form.stateId !== null) payload.state_id = form.stateId;
  if (form.priority !== "none") payload.priority = form.priority as NonNullable<IssueCreateInput["priority"]>;
  if (form.assigneeIds.length > 0) payload.assignee_ids = [...form.assigneeIds];
  if (form.labelIds.length > 0) payload.label_ids = [...form.labelIds];
  if (form.startDate !== null) payload.start_date = form.startDate;
  if (form.targetDate !== null) payload.target_date = form.targetDate;
  if (form.estimatePoint !== null) payload.estimate_point = form.estimatePoint;
  if (form.parentId !== null) payload.parent_id = form.parentId;
  if (form.execution !== null) {
    const target = executionTargetPatch(form.execution);
    payload.assigned_pod_id = target.assigned_pod_id;
    payload.agent_executor = target.agent_executor;
  }
  const branch = form.gitWorkBranch.trim();
  if (branch !== "") payload.git_work_branch = branch;
  return payload;
}

/**
 * Membership the item needs after it exists (ISS-117). The create
 * endpoint does not take a cycle or module, so the submit attaches them
 * through the bridge writes once the server answers with the row id.
 */
export interface CreateMembership {
  cycleId: string | null;
  moduleIds: string[];
}

export function membershipForForm(form: CreateFormState): CreateMembership {
  return { cycleId: form.cycleId, moduleIds: [...form.moduleIds] };
}

export function hasMembership(membership: CreateMembership): boolean {
  return membership.cycleId !== null || membership.moduleIds.length > 0;
}

/** Draft input for the guard's Save-to-drafts action (ISS-120). */
export function buildDraftInput(form: CreateFormState): DraftIssueCreateInput {
  const trimmed = normalizeTitle(form.name);
  const draft: DraftIssueCreateInput = {
    name: trimmed === "" ? UNTITLED_DRAFT_NAME : trimmed,
    description_html: visibleDescriptionHtml(form.descriptionHtml),
  };
  // The draft belongs to the project the form was filing under; the
  // server links it from the request body and validates the other
  // fields against it (ISS-120, ISS-138/ISS-139).
  if (form.projectId !== null) draft.project_id = form.projectId;
  if (form.stateId !== null) draft.state_id = form.stateId;
  if (form.priority !== "none") draft.priority = form.priority as NonNullable<DraftIssueCreateInput["priority"]>;
  if (form.assigneeIds.length > 0) draft.assignee_ids = [...form.assigneeIds];
  if (form.labelIds.length > 0) draft.label_ids = [...form.labelIds];
  if (form.startDate !== null) draft.start_date = form.startDate;
  if (form.targetDate !== null) draft.target_date = form.targetDate;
  if (form.parentId !== null) draft.parent_id = form.parentId;
  return draft;
}

/**
 * Whether the discard guard has anything worth keeping (ISS-120). The
 * project is context, not content, so it never counts; empty values and
 * a description without visible text do not either.
 */
export function hasDraftWorthKeeping(form: CreateFormState): boolean {
  if (normalizeTitle(form.name) !== "") return true;
  if (!isStoredHtmlEmpty(form.descriptionHtml)) return true;
  if (form.stateId !== null) return true;
  if (form.priority !== "none") return true;
  if (form.assigneeIds.length > 0) return true;
  if (form.labelIds.length > 0) return true;
  if (form.startDate !== null || form.targetDate !== null) return true;
  if (form.cycleId !== null) return true;
  if (form.moduleIds.length > 0) return true;
  if (form.estimatePoint !== null) return true;
  if (form.execution !== null) return true;
  if (form.parentId !== null) return true;
  if (form.gitWorkBranch.trim() !== "") return true;
  return false;
}
