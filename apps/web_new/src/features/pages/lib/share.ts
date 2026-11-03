// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row sharing helpers (parity/specs/pages.md §3.8, PAGE-006/010). The
// list's viewer-safe actions open the canonical detail address or copy
// it; the copy text and failure are reported through the app's single
// notice surface.

import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";

export const PAGE_LINK_COPIED_TITLE = "Page link copied to clipboard.";
export const PAGE_LINK_COPY_FAILED_TITLE = "Couldn't copy the page link.";

export function pageLinkUrl(workspaceSlug: string, projectId: string, pageId: string): string {
  const path = `/${workspaceSlug}/projects/${projectId}/pages/${pageId}`;
  return new URL(path, window.location.origin).toString();
}

/** Write the canonical page address; true when the clipboard accepts it. */
export async function copyPageLink(workspaceSlug: string, projectId: string, pageId: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(pageLinkUrl(workspaceSlug, projectId, pageId));
    notifySuccess(PAGE_LINK_COPIED_TITLE);
    return true;
  } catch {
    notifyError(PAGE_LINK_COPY_FAILED_TITLE);
    return false;
  }
}
