// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export dialog registration (PAGE-074): importing the chunk puts
// the exports area's dialog into the detail seam.

import { afterEach, describe, expect, it } from "vitest";

import { PageExportDialog } from "./components/PageExportDialog.js";
import { getPageExportDialog, resetPageExportDialog } from "./lib/pageExportDialogRegistry.js";
import { registerPageExport } from "./register.js";

afterEach(() => {
  resetPageExportDialog();
});

describe("pages-export register", () => {
  it("registers the dialog on import", () => {
    expect(getPageExportDialog()).toBe(PageExportDialog);
  });

  it("re-registers on demand and unregisters on cleanup", () => {
    resetPageExportDialog();
    expect(getPageExportDialog()).toBeNull();
    const unregister = registerPageExport();
    expect(getPageExportDialog()).toBe(PageExportDialog);
    unregister();
    expect(getPageExportDialog()).toBeNull();
  });
});
