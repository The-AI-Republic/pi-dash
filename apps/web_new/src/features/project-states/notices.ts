// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Notice copy and refusal classification for the States tab
// (parity/specs/project-settings-states.md, PSET-029–035). Mutations
// answer through toasts only — the tab has no error panel or retry
// control — so every failure the operator can see is one of these.

import { isApiError } from "@pidash/api-client";

export const STATE_CREATED_TITLE = "State created successfully";
export const STATE_CREATE_FAILED_TITLE = "State could not be created";
export const STATE_DUPLICATE_CREATE_MESSAGE = "Another state exists with the same name. Please try a different name.";

export const STATE_UPDATED_TITLE = "State updated successfully";
export const STATE_UPDATE_FAILED_TITLE = "State could not be updated";
export const STATE_DUPLICATE_RENAME_MESSAGE = "Another state exists with the same name. Please try a different name.";

export const STATE_DELETED_TITLE = "State deleted successfully";
/** The server refuses a state that still holds work items; guide the operator. */
export const STATE_DELETE_OCCUPIED_TITLE = "State could not be deleted";
export const STATE_DELETE_OCCUPIED_MESSAGE =
  "This state still has work items. Move them to another state first, then delete it.";
export const STATE_DELETE_FAILED_TITLE = "State could not be deleted";

export const STATE_DEFAULT_TITLE = "Default state updated";
export const STATE_DEFAULT_FAILED_TITLE = "State could not be made the default";

export const MEMBER_EDITING_ON_TITLE = "Members can now edit states";
export const MEMBER_EDITING_OFF_TITLE = "Members can no longer edit states";
export const MEMBER_EDITING_FAILED_TITLE = "Couldn't change the member editing setting";

/** Fallback detail for a refusal with no usable message. */
export const STATE_FAILURE_FALLBACK = "Check your connection and try again.";

/**
 * True when the server refused a create/rename as a duplicate name. The
 * state views answer an integrity clash with `{"name": "<sentence>"}` — a
 * bare string, not DRF's per-field list — so the client keeps the raw
 * body too and reads the name key from either shape. A DRF `name` field
 * error (array) answers the same refusal.
 */
export function isDuplicateStateError(error: unknown): boolean {
  if (!isApiError(error) || error.status !== 400) return false;
  if (error.fields !== undefined && Object.keys(error.fields).includes("name")) return true;
  if (error.body === null || typeof error.body !== "object") return false;
  const record = error.body as Record<string, unknown>;
  return Object.keys(record).length > 0 && Object.keys(record).every((key) => key === "name");
}

/**
 * True when a delete was refused because issues still live in the
 * state. The server answers `{"error": "The state is not empty, only
 * empty states can be deleted"}`; an equivalent refusal that talks about
 * work items in use counts too. The wording is matched because the
 * status alone cannot tell this from the other 400s (default state,
 * validation).
 */
export function isOccupiedStateError(error: unknown): boolean {
  if (!isApiError(error)) return false;
  return /not empty|move|in use|issue/i.test(error.message);
}

/** Detail for a failure notice: the server's message, else the fallback. */
export function stateFailureDetail(error: unknown): string {
  if (isApiError(error) && error.message !== "") return error.message;
  return STATE_FAILURE_FALLBACK;
}
