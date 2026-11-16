// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sharing one module's address (parity/specs/modules.md §4.6, MOD-018).
// The one copy helper for the area: the row/card menu and the detail
// header menu call it, and the archived screen reuses the same menu so
// it needs no second copy. Row copy always clips the canonical detail
// address — never the current address with its query — and confirms
// visibly; a refused clipboard reports the failure instead of claiming
// success. Opening in a new tab goes through the shared shell seam, so
// the desktop bundle hands out the hosted app's address.

import { copyLinkWithToast, openShareAddress } from "../../../shared/shell/shareLink.js";
import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";
import { moduleDetailHref } from "./moduleList.js";
import { MODULE_FAILURE_TITLE, MODULE_SUCCESS_TITLE } from "./moduleForm.js";

/** Pinned confirmation the oracle reads in the toast body (MOD-018). */
export const MODULE_LINK_COPIED_BODY = "Module link copied to clipboard.";

export const MODULE_OPEN_NEW_TAB_FAILED_TITLE = "Couldn't open the module in a new tab";

/**
 * Copy one module's canonical address to the clipboard with toast
 * confirmation. Never rejects: resolves true when the write lands.
 */
export async function copyModuleLink(workspaceSlug: string, projectId: string, moduleId: string): Promise<boolean> {
  return copyLinkWithToast(
    moduleDetailHref(workspaceSlug, projectId, moduleId),
    { copied: MODULE_SUCCESS_TITLE, copiedBody: MODULE_LINK_COPIED_BODY, failed: MODULE_FAILURE_TITLE },
    { success: notifySuccess, error: notifyError }
  );
}

/**
 * Open one module's canonical address in a new tab, leaving the current
 * view untouched. A refused open toasts the failure instead of throwing.
 */
export async function openModuleInNewTab(workspaceSlug: string, projectId: string, moduleId: string): Promise<void> {
  try {
    await openShareAddress(moduleDetailHref(workspaceSlug, projectId, moduleId));
  } catch (error) {
    notifyError(MODULE_OPEN_NEW_TAB_FAILED_TITLE, error instanceof Error ? error.message : undefined);
  }
}
