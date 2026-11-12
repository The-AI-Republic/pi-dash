// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Notices and addresses the quick-action menus share
// (parity/specs/issues.md §7, ISS-060–067). The copy-link title and the
// list link's toast body are fixed by the parity scenarios; the rest
// name the record kind so the same wording serves issues and epics.

import { workItemPath } from "../../../shared/shell/shareLink.js";

export const COPY_LINK_COPIED_TITLE = "Link copied";
export const COPY_LINK_COPY_FAILED_TITLE = "Couldn't copy the link";

/** Toasts after a restore from the archived list or header (ISS-064/066). */
export const RESTORE_SUCCESS_TITLE = "Restore success";
export const RESTORE_SUCCESS_BODY = "The work item is back in its project's active lists.";
export const RESTORE_FAILED_TITLE = "Couldn't restore the work item";
export const RESTORE_FAILED_BODY = "Check your connection and try again.";

/** Toasts after removing a row from its cycle or module (ISS-062/063). */
export const REMOVE_FAILED_TITLE = "Couldn't remove the work item";
export const REMOVE_FAILED_BODY = "Check your connection and try again.";

/** Toasts after copying the whole list's address (ISS-067). */
export const LIST_COPY_MESSAGE = "The list link copied to clipboard.";
export const LIST_COPY_FAILED_TITLE = "Couldn't copy the list link";

/** The browse address for a row's short key, e.g. "/acme/browse/WEB-12/". */
export function workItemBrowseHref(workspaceSlug: string, projectIdentifier: string, sequenceId: number): string {
  return workItemPath(workspaceSlug, `${projectIdentifier}-${sequenceId}`);
}

/** Success title for a membership removal; the noun names the context. */
export function removeSuccessTitle(kind: "cycle" | "module"): string {
  return kind === "cycle" ? "Removed from cycle" : "Removed from module";
}

/** The menu leading mark's accessible description for a whole-list menu. */
export const LIST_MENU_LABEL = "List actions";
