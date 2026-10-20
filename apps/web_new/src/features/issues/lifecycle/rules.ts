// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lifecycle rules (parity/specs/issues.md §11, ISS-134–136): the copy and
// admission decisions behind the archive, delete and move confirmations.
// The dialogs render these strings; keeping them here lets the tests pin
// the exact sentences the oracle reads without starting a dialog.

import { ROLE_ADMIN } from "../../../core/session/permissions.js";

/** What kind of record a confirmation is about. */
export type WorkItemKind = "issue" | "sub_issue" | "epic";

/** The noun a dialog uses for its record: "work item", "sub-work item", "epic". */
export function kindNoun(kind: WorkItemKind): string {
  switch (kind) {
    case "epic":
      return "epic";
    case "sub_issue":
      return "sub-work item";
    default:
      return "work item";
  }
}

/** Title case of the noun, for headings and success lines. */
export function kindTitle(kind: WorkItemKind): string {
  const noun = kindNoun(kind);
  return noun.charAt(0).toUpperCase() + noun.slice(1);
}

/** The key people recognise the record by, e.g. "WEB-42". */
export function workItemKey(projectIdentifier: string | null | undefined, sequenceId: number): string {
  return `${projectIdentifier ?? ""}-${sequenceId}`;
}

/**
 * Archive heading: the verb plus the record's identifier and sequence,
 * separated by a space ("Archive work item WEB 42"). The archive dialog
 * names the sequence on its own; the other confirmations use the dashed
 * short key.
 */
export function archiveHeading(
  kind: WorkItemKind,
  projectIdentifier: string | null | undefined,
  sequenceId: number
): string {
  return `Archive ${kindNoun(kind)} ${projectIdentifier ?? ""} ${sequenceId}`;
}

/** Archive body: the record can be brought back from the project archives. */
export function archiveBody(kind: WorkItemKind): string {
  return `Archive this ${kindNoun(kind)}? Archived ${kindNoun(kind)}s can be restored later from the project archives.`;
}

/** Delete heading: the verb plus the record's noun. */
export function deleteHeading(kind: WorkItemKind): string {
  return `Delete ${kindNoun(kind)}`;
}

/** Delete body: names the short key and warns the removal is permanent. */
export function deleteBody(kind: WorkItemKind, key: string): string {
  return `Delete ${key}? All of the data related to this ${kindNoun(kind)} will be permanently removed. This action cannot be undone.`;
}

/** Confirmation button label while the write runs. */
export function pendingLabel(verb: "archive" | "delete", pending: boolean): string {
  if (!pending) return verb === "archive" ? "Archive" : "Delete";
  return verb === "archive" ? "Archiving…" : "Deleting…";
}

/** Failure/success copy shared by the delete flow. */
export const DELETE_SUCCESS_TITLE = "Success!";
export const DELETE_PERMISSION_TITLE = "You don't have permission to perform this action.";
export const DELETE_FAILED_TITLE = "Error";
export const DELETE_FAILED_BODY = "Failed to delete work item";
export const ARCHIVE_SUCCESS_TITLE = "Archive success";
export const ARCHIVE_SUCCESS_BODY = "Your archives can be found in project archives.";
export const ARCHIVE_FAILED_TITLE = "Error while archiving";
export const ARCHIVE_FAILED_BODY = "The work item could not be archived. Please try again.";
export const MOVE_SUCCESS_TITLE = "Success!";
export const MOVE_SUCCESS_BODY = "Work item moved successfully.";
export const MOVE_FAILED_TITLE = "Error while moving";
export const MOVE_FAILED_BODY = "The work item could not be moved. Please try again.";
export const MOVE_EMPTY_NOTE = "No other projects available to move this work item to.";
export const MOVE_SEARCH_PLACEHOLDER = "Search for a project...";

/** Success line after a delete: "{noun} deleted successfully". */
export function deleteSuccessBody(kind: WorkItemKind): string {
  return `${kindNoun(kind)} deleted successfully`;
}

/** Who may delete a record: its creator or a project admin (ISS-135). */
export interface DeleteStanding {
  /** The signed-in user's id, when known. */
  viewerId: string | null | undefined;
  /** The record creator's id, when the server sent one. */
  creatorId: string | null | undefined;
  /** The viewer's role on the owning project. */
  projectRole: number | null | undefined;
}

/**
 * Creator-or-admin standing. An unknown viewer (still loading) fails
 * closed, so a click before the session resolves never sends the delete
 * the server would refuse anyway.
 */
export function mayDeleteWorkItem({ viewerId, creatorId, projectRole }: DeleteStanding): boolean {
  if (viewerId === null || viewerId === undefined || viewerId === "") return false;
  if (creatorId !== null && creatorId !== undefined && creatorId !== "" && creatorId === viewerId) return true;
  return projectRole !== null && projectRole !== undefined && projectRole >= ROLE_ADMIN;
}

/**
 * The server's permission refusal, recognised by status or by the
 * endpoint's own message, maps to the shared permission notice (ISS-135).
 */
export function isDeletePermissionError(error: unknown): boolean {
  if (typeof error !== "object" || error === null) return false;
  const status = (error as { status?: unknown }).status;
  if (status === 403) return true;
  const message = (error as { message?: unknown }).message;
  return typeof message === "string" && /only admin or creator/i.test(message);
}

/** Server detail for a failed write, falling back to the dialog's copy. */
export function failureDetail(error: unknown, fallback: string): string {
  if (typeof error === "object" && error !== null) {
    const detail = (error as { detail?: unknown }).detail;
    if (typeof detail === "string" && detail.trim() !== "") return detail.trim();
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim() !== "") return message.trim();
  }
  return fallback;
}

/** One project a move may target. */
export interface MovableProject {
  id: string;
  name: string;
  identifier: string;
}

/**
 * Move targets: the projects the viewer belongs to, minus the current
 * one and archived projects, in the caller's order (ISS-136). A blank or
 * unresolved row never renders (it would also match every query).
 */
export function moveTargets(projects: readonly MovableProject[], currentProjectId: string): MovableProject[] {
  return projects.filter((project) => project.id !== currentProjectId && project.identifier !== "");
}

/**
 * Filter targets by a query over the name and the identifier (the old
 * dialog's case-insensitive substring match).
 */
export function filterMoveTargets(projects: readonly MovableProject[], query: string): MovableProject[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return [...projects];
  return projects.filter((project) => `${project.identifier} ${project.name}`.toLowerCase().includes(needle));
}

/** Only a finished record archives; the endpoint answers 400 otherwise. */
export const ARCHIVABLE_STATE_GROUPS: readonly string[] = ["completed", "cancelled"];

/** Whether a record in `stateGroup` may be archived. */
export function isArchivable(stateGroup: string | null | undefined): boolean {
  return stateGroup !== null && stateGroup !== undefined && ARCHIVABLE_STATE_GROUPS.includes(stateGroup);
}

/** The browse address for a moved record, e.g. "/acme/browse/WEB-9/". */
export function movedWorkItemPath(workspaceSlug: string, projectIdentifier: string, sequenceId: number): string {
  return `/${workspaceSlug}/browse/${projectIdentifier}-${sequenceId}/`;
}
