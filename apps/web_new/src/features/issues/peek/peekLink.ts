// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Card link copy (moved from the kanban minimal peek; the card menu
// keeps it while the full peek owns the panel). Copies one work item's
// address and reports through the shell toasts.

import { copyWorkItemLink, workItemPath } from "../../../shared/shell/shareLink.js";

/** The two copy-link notices the card action reports. */
export const CARD_COPY_COPIED = "Copied the work item link.";
export const CARD_COPY_FAILED = "Couldn't copy the work item link.";

/** Copies one work item's address and reports through the shell toasts. */
export function copyCardLink(workspaceSlug: string, issueKey: string): Promise<boolean> {
  return copyWorkItemLink(workItemPath(workspaceSlug, issueKey), {
    copied: CARD_COPY_COPIED,
    failed: CARD_COPY_FAILED,
    failedBody: "Check the clipboard permission and try again.",
  });
}
