// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sharing one cycle's address (parity/specs/cycles.md §19, CYC-046).
// The one copy helper for the area: the row menu (slice E) and the
// palette registration (slice H) call it, and the peek wires it now.
// Row copy always clips the canonical detail address — never the
// current address with its query — and confirms visibly; a refused
// clipboard reports the failure instead of claiming success. Opening
// in a new tab goes through the active platform, so the desktop
// bundle hands out the hosted app's address.

import { platform } from "../../../core/platform/index.js";
import { copyLinkWithToast, shareableAddress } from "../../../shared/shell/shareLink.js";
import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";
import { cycleDetailPath } from "./cycleScreen.js";

export const CYCLE_LINK_COPIED_TITLE = "Cycle link copied";
export const CYCLE_LINK_COPY_FAILED_TITLE = "Couldn't copy the cycle link";
export const CYCLE_OPEN_NEW_TAB_FAILED_TITLE = "Couldn't open the cycle in a new tab";

/**
 * Copy one cycle's canonical address to the clipboard with toast
 * confirmation. Never rejects: resolves true when the write lands.
 */
export async function copyCycleLink(workspaceSlug: string, projectId: string, cycleId: string): Promise<boolean> {
  return copyLinkWithToast(
    cycleDetailPath(workspaceSlug, projectId, cycleId),
    { copied: CYCLE_LINK_COPIED_TITLE, failed: CYCLE_LINK_COPY_FAILED_TITLE },
    { success: notifySuccess, error: notifyError }
  );
}

/**
 * Open one cycle's canonical address in a new tab, leaving the current
 * view untouched. A refused open toasts the failure instead of throwing.
 */
export async function openCycleInNewTab(workspaceSlug: string, projectId: string, cycleId: string): Promise<void> {
  try {
    await platform.openExternal(shareableAddress(cycleDetailPath(workspaceSlug, projectId, cycleId)));
  } catch (error) {
    notifyError(CYCLE_OPEN_NEW_TAB_FAILED_TITLE, error instanceof Error ? error.message : undefined);
  }
}
