// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake create-dialog form rules (parity/specs/intake.md INT-013–015).
// Pure state shaping only: the dialog component owns focus and the
// network calls stay in api/, while these helpers decide what a save
// submits, when a title blocks, and what the form looks like after a
// create-with-more reset. The validation split is deliberate: an
// over-long title never leaves the dialog, while an empty title and an
// out-of-range priority travel to the server, which refuses them.

import type { IntakeIssueCreateInput } from "@pidash/api-client";

import { hasStoredDescriptionContent } from "../../../shared/editor/index.js";
import type { PriorityLevel } from "../../../shared/pickers/index.js";

/** The server's name cap, shared with the work-item form. */
export const INTAKE_TITLE_MAX_LENGTH = 255;

/** Everything the create dialog holds while it is open. */
export interface IntakeCreateForm {
  name: string;
  descriptionHtml: string;
  /** Committed intake-state pick; null shows the project's triage state. */
  stateId: string | null;
  priority: PriorityLevel;
  assigneeIds: string[];
  labelIds: string[];
  /** Due date as an ISO day, or null for none. */
  targetDate: string | null;
}

/** A blank form. `descriptionHtml` seeds the editor's default content. */
export function emptyIntakeCreateForm(descriptionHtml = ""): IntakeCreateForm {
  return {
    name: "",
    descriptionHtml,
    stateId: null,
    priority: "none",
    assigneeIds: [],
    labelIds: [],
    targetDate: null,
  };
}

/**
 * The form after a successful save while Create another is on
 * (INT-015). Nothing about the project or the tab changes — the dialog
 * never leaves them — so every field returns to its create default and
 * the unset state shows the triage fallback again.
 */
export function resetIntakeCreateForm(descriptionDefault = ""): IntakeCreateForm {
  return emptyIntakeCreateForm(descriptionDefault);
}

/** Trimmed title, or empty when the field holds only whitespace. */
export function normalizeIntakeTitle(name: string): string {
  return name.trim();
}

/**
 * Inline title message, or null when the field may be sent (INT-014).
 * Only length blocks: an empty title is the server's refusal to make,
 * so the dialog posts it rather than second-guessing the rule.
 */
export function intakeTitleError(name: string): string | null {
  if (name.length > INTAKE_TITLE_MAX_LENGTH) {
    return `Titles stop at ${INTAKE_TITLE_MAX_LENGTH} characters.`;
  }
  return null;
}

/** True when the title may be submitted as typed (INT-014). */
export function isIntakeTitleSubmittable(name: string): boolean {
  return intakeTitleError(name) === null;
}

/**
 * The stored description, or an empty string when it holds no content.
 * Text, images and mentions all count (the shared rule); the editor's
 * mount-time empty paragraph is not something the user typed and must
 * not travel in the create payload.
 */
export function visibleIntakeDescriptionHtml(descriptionHtml: string): string {
  return hasStoredDescriptionContent(descriptionHtml) ? descriptionHtml : "";
}

/**
 * The create payload: always the title and the state the picker shows
 * (empty while unset — the server files the request in triage either
 * way), plus every property the user set. The shared work-item form's
 * start date, cycle, module, estimate and parent stay out: the old
 * properties strip hides those controls here and submits none of them.
 */
export function buildIntakeCreatePayload(form: IntakeCreateForm): IntakeIssueCreateInput {
  const issue: IntakeIssueCreateInput["issue"] = {
    name: normalizeIntakeTitle(form.name),
    state_id: form.stateId ?? "",
  };
  const description = visibleIntakeDescriptionHtml(form.descriptionHtml);
  if (description !== "") issue.description_html = description;
  if (form.priority !== "none") issue.priority = form.priority;
  if (form.assigneeIds.length > 0) issue.assignee_ids = [...form.assigneeIds];
  if (form.labelIds.length > 0) issue.label_ids = [...form.labelIds];
  if (form.targetDate !== null) issue.target_date = form.targetDate;
  return { source: "IN_APP", issue };
}
