// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Exports guide registration (EXP-001): importing the chunk puts the
// guide into the workspace-settings exports seam, and the exported
// helper can put it back after a reset.
import { afterEach, describe, expect, it } from "vitest";

import { getWorkspaceExportsGuide, resetWorkspaceExportsGuide } from "../workspace-settings/index.js";
import { ExportsGuide } from "./components/ExportsGuide.js";
import { registerExportsGuide } from "./register.js";

afterEach(() => {
  resetWorkspaceExportsGuide();
});

describe("exports guide registration", () => {
  it("registers the guide into the settings seam on import", () => {
    expect(getWorkspaceExportsGuide()).toBe(ExportsGuide);
  });

  it("registers again after a reset and returns an unregister cleanup", () => {
    resetWorkspaceExportsGuide();
    expect(getWorkspaceExportsGuide()).toBeNull();
    const unregister = registerExportsGuide();
    expect(getWorkspaceExportsGuide()).toBe(ExportsGuide);
    unregister();
    expect(getWorkspaceExportsGuide()).toBeNull();
  });
});
