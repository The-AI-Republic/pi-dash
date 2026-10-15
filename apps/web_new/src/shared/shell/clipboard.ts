// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Copy-current-page-URL action (shell spec, SHELL-089). The palette entry
// and its chord run this: the address lands on the clipboard with a
// confirming toast, and a denied write toasts the failure instead.

import { notifyError, notifySuccess } from "./toaster.js";

export const PAGE_URL_COPIED_TITLE = "Current page URL copied to clipboard.";
export const PAGE_URL_COPY_FAILED_TITLE = "Couldn't copy the page URL";

/** Copy the current address to the clipboard, toasting either outcome. */
export async function copyCurrentPageUrl(): Promise<void> {
  try {
    await navigator.clipboard.writeText(window.location.href);
    notifySuccess(PAGE_URL_COPIED_TITLE);
  } catch (error) {
    notifyError(PAGE_URL_COPY_FAILED_TITLE, (error as Error).message);
  }
}
