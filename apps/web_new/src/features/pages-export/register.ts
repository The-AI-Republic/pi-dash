// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export dialog registration (NEWFRONT-770 seam, PAGE-074). Importing
// this chunk puts the dialog into the detail seam; the pages detail route
// lazy-loads it so the dialog stays out of the initial bundle. The
// exported helper lets tests re-establish the registration after a reset.
import { PageExportDialog } from "./components/PageExportDialog.js";

import { registerPageExportDialog } from "./lib/pageExportDialogRegistry.js";

/** Registers the page-export dialog in the detail seam. */
export function registerPageExport(): () => void {
  return registerPageExportDialog(PageExportDialog);
}

registerPageExport();
