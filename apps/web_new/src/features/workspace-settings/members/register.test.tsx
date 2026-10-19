// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members registration (WSET-001/008): importing the chunk registers the
// roster body under the members section, and the exported helper can put
// it back after a reset.
import { afterEach, describe, expect, it } from "vitest";

import { getWorkspaceSettingsPage, resetWorkspaceSettingsPages } from "../lib/registry.js";
import { MembersPage } from "./MembersPage.js";
import { registerMembersSettingsPage } from "./register.js";

afterEach(() => {
  resetWorkspaceSettingsPages();
});

describe("members page registration", () => {
  it("registers the roster as the members section body on import", () => {
    expect(getWorkspaceSettingsPage("members")).toBe(MembersPage);
  });

  it("registers again after a reset and returns an unregister callback", () => {
    resetWorkspaceSettingsPages();
    expect(getWorkspaceSettingsPage("members")).toBeNull();
    const unregister = registerMembersSettingsPage();
    expect(getWorkspaceSettingsPage("members")).toBe(MembersPage);
    unregister();
    expect(getWorkspaceSettingsPage("members")).toBeNull();
  });
});
