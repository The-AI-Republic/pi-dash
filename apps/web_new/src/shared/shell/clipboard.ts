// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Copy-current-page-URL action (shell spec, SHELL-089). The palette entry
// and its chord run this: the address lands on the clipboard with a
// confirming toast, and a denied write toasts the failure instead. The
// address resolves through the platform so the desktop bundle copies the
// hosted app's address, not its private bundle origin; a refusal (no
// hosted origin configured) toasts the failure rather than writing it.

import { copyShareAddress } from "./shareLink.js";
import { notifyError, notifySuccess } from "./toaster.js";

export const PAGE_URL_COPIED_TITLE = "Current page URL copied to clipboard.";
export const PAGE_URL_COPY_FAILED_TITLE = "Couldn't copy the page URL";

/** The current page as a site-relative path, keeping query and fragment. */
function currentPagePath(): string {
  const location = globalThis.window?.location;
  if (location === undefined) throw new Error("The current page address is not available in this window.");
  return `${location.pathname}${location.search ?? ""}${location.hash ?? ""}`;
}

/** Copy the current address to the clipboard, toasting either outcome. */
export async function copyCurrentPageUrl(): Promise<void> {
  try {
    await copyShareAddress(currentPagePath());
    notifySuccess(PAGE_URL_COPIED_TITLE);
  } catch (error) {
    notifyError(PAGE_URL_COPY_FAILED_TITLE, (error as Error).message);
  }
}
