// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Built-in tab registration (parity/specs/profile.md §5/§7/§9): the general,
// preferences, security, token, notifications, activity, assistant, upkeep
// and integrations bodies are present on a direct visit, and unmounting
// them leaves the seam to the sibling slices.
import { afterEach, describe, expect, it } from "vitest";

import { getProfileSettingsPage, resetProfileSettingsPages } from "./lib/registry.js";
import { registerBuiltInSettingsTabs } from "./pageEntries.js";

afterEach(() => resetProfileSettingsPages());

describe("built-in settings tabs", () => {
  it("registers all eight built-in bodies", () => {
    expect(getProfileSettingsPage("general")).not.toBeNull();
    expect(getProfileSettingsPage("preferences")).not.toBeNull();
    expect(getProfileSettingsPage("security")).not.toBeNull();
    expect(getProfileSettingsPage("api-tokens")).not.toBeNull();
    expect(getProfileSettingsPage("notifications")).not.toBeNull();
    expect(getProfileSettingsPage("activity")).not.toBeNull();
    expect(getProfileSettingsPage("ai-assistant")).not.toBeNull();
    expect(getProfileSettingsPage("auto-project-management")).not.toBeNull();
    expect(getProfileSettingsPage("integrations")).not.toBeNull();
  });

  it("unregisters every body on demand", () => {
    const unregister = registerBuiltInSettingsTabs();
    unregister();
    expect(getProfileSettingsPage("general")).toBeNull();
    expect(getProfileSettingsPage("preferences")).toBeNull();
    expect(getProfileSettingsPage("security")).toBeNull();
    expect(getProfileSettingsPage("api-tokens")).toBeNull();
    expect(getProfileSettingsPage("notifications")).toBeNull();
    expect(getProfileSettingsPage("activity")).toBeNull();
    expect(getProfileSettingsPage("ai-assistant")).toBeNull();
    expect(getProfileSettingsPage("auto-project-management")).toBeNull();
    expect(getProfileSettingsPage("integrations")).toBeNull();
  });
});
