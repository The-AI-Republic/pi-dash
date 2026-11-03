// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dialog dismissal rule (WSET-037, spec §37): Escape and the backdrop
// close ordinary settings dialogs, while a secret reveal only closes
// from its explicit action.
import { describe, expect, it } from "vitest";

import { nextWorkspaceSettingsDialogOpen } from "./dialog.js";

describe("nextWorkspaceSettingsDialogOpen", () => {
  it("lets a reveal dialog open", () => {
    expect(nextWorkspaceSettingsDialogOpen(false, { open: true, reason: "trigger-press" }, { reveal: true })).toBe(
      true
    );
  });

  it("closes an ordinary dialog on every dismissal reason", () => {
    for (const reason of ["escape-key", "outside-press", "close-press", "focus-out"]) {
      expect(nextWorkspaceSettingsDialogOpen(true, { open: false, reason }, { reveal: false })).toBe(false);
    }
  });

  it("keeps a reveal dialog open on every dismissal reason", () => {
    for (const reason of ["escape-key", "outside-press", "close-press", "focus-out"]) {
      expect(nextWorkspaceSettingsDialogOpen(true, { open: false, reason }, { reveal: true })).toBe(true);
    }
  });
});
