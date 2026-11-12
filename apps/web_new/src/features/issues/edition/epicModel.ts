// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Epic dialog shaping (ISS-232). Epics are work items stored under their
// own collection, so the dialog edits the same core fields: name,
// description, state, priority, assignees, labels and the date window. An
// empty title is refused locally; nothing else is required.

import type { Issue, IssueCreateInput, IssuePatch } from "@pidash/api-client";

/** The five priority levels the picker offers (server vocabulary). */
export type EpicPriority = NonNullable<IssuePatch["priority"]>;

/** Everything the epic dialog holds while it is open. */
export interface EpicDialogValues {
  name: string;
  descriptionHtml: string;
  stateId: string | null;
  priority: EpicPriority;
  assigneeIds: string[];
  labelIds: string[];
  startDate: string | null;
  targetDate: string | null;
}

/** The default priority for a new epic: the server's "none". */
export const DEFAULT_EPIC_PRIORITY: EpicPriority = "none";

/** The form's starting values: the epic's row, or a blank create. */
export function epicDraftFrom(epic: Issue | undefined): EpicDialogValues {
  if (epic === undefined) {
    return {
      name: "",
      descriptionHtml: "",
      stateId: null,
      priority: DEFAULT_EPIC_PRIORITY,
      assigneeIds: [],
      labelIds: [],
      startDate: null,
      targetDate: null,
    };
  }
  return {
    name: epic.name,
    descriptionHtml: epic.description_html,
    stateId: epic.state_id,
    priority: epic.priority,
    assigneeIds: [...epic.assignee_ids],
    labelIds: [...epic.label_ids],
    startDate: epic.start_date ?? null,
    targetDate: epic.target_date ?? null,
  };
}

/** The editable part of the form for an epics PATCH. */
export function epicPatchOf(values: EpicDialogValues): IssuePatch {
  return {
    name: values.name.trim(),
    description_html: values.descriptionHtml,
    state_id: values.stateId,
    priority: values.priority,
    assignee_ids: values.assigneeIds,
    label_ids: values.labelIds,
    start_date: values.startDate,
    target_date: values.targetDate,
  };
}

/**
 * The create payload. The epics collection assigns its own rank, so no
 * sort order travels; an unset state lets the server pick the default.
 */
export function epicCreateOf(values: EpicDialogValues): IssueCreateInput {
  const trimmed = values.name.trim();
  return {
    name: trimmed,
    description_html: values.descriptionHtml,
    priority: values.priority,
    assignee_ids: values.assigneeIds,
    label_ids: values.labelIds,
    start_date: values.startDate,
    target_date: values.targetDate,
    ...(values.stateId === null ? {} : { state_id: values.stateId }),
  };
}

/** Dialog heading for the mode. */
export function epicTitle(editing: boolean): string {
  return editing ? "Edit epic" : "Create epic";
}

/** The sentence a nameless form shows beside the name field. */
export const EPIC_NAME_REQUIRED = "Give the epic a name.";

/** Whether a form may submit: a non-blank name. */
export function canSubmitEpic(values: EpicDialogValues): boolean {
  return values.name.trim() !== "";
}

/** The name field's inline error, or undefined while it is valid. */
export function epicNameError(values: EpicDialogValues): string | undefined {
  return canSubmitEpic(values) ? undefined : EPIC_NAME_REQUIRED;
}
