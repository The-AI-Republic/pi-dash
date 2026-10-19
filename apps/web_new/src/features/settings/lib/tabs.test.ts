// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  DEFAULT_PROFILE_SETTINGS_TAB,
  findProfileSettingsTab,
  isProfileSettingsTab,
  profileSettingsGroups,
  profileSettingsHref,
  profileSettingsTitle,
  PROFILE_SETTINGS_TABS,
  queryWithoutMark,
  resolveProfileSettingsTab,
} from "./tabs.js";

describe("profile settings tabs", () => {
  it("registers the nine tab ids with their grouped labels in order", () => {
    expect(PROFILE_SETTINGS_TABS.map((tab) => tab.id)).toEqual([
      "general",
      "preferences",
      "ai-assistant",
      "auto-project-management",
      "notifications",
      "security",
      "activity",
      "integrations",
      "api-tokens",
    ]);
    expect(profileSettingsGroups().map((group) => group.label)).toEqual(["Your Profile", "Developer"]);
    expect(profileSettingsGroups().map((group) => group.tabs.map((tab) => tab.label))).toEqual([
      ["Profile", "Preferences", "AI Assistant", "Auto Project Management", "Notifications", "Security", "Activity"],
      ["Integrations", "Personal Access Tokens"],
    ]);
  });

  it("recognizes only registered ids", () => {
    expect(isProfileSettingsTab("security")).toBe(true);
    expect(isProfileSettingsTab("bogus-tab-id")).toBe(false);
  });

  it("resolves an address segment, tolerating a trailing slash", () => {
    expect(resolveProfileSettingsTab("api-tokens")).toBe("api-tokens");
    expect(resolveProfileSettingsTab("api-tokens/")).toBe("api-tokens");
    expect(resolveProfileSettingsTab("bogus")).toBeNull();
    expect(resolveProfileSettingsTab(undefined)).toBeNull();
    expect(DEFAULT_PROFILE_SETTINGS_TAB).toBe("general");
  });

  it("builds tab links with a trailing slash and keeps a query verbatim", () => {
    expect(profileSettingsHref("general")).toBe("/settings/profile/general/");
    expect(profileSettingsHref("security", "?keep=2")).toBe("/settings/profile/security/?keep=2");
    expect(profileSettingsHref("security", "keep=2")).toBe("/settings/profile/security/?keep=2");
    expect(profileSettingsHref("security", "?")).toBe("/settings/profile/security/");
    expect(queryWithoutMark("?a=1")).toBe("a=1");
    expect(queryWithoutMark("a=1")).toBe("a=1");
  });

  it("finds a tab by id and titles the document", () => {
    expect(findProfileSettingsTab("notifications").label).toBe("Notifications");
    expect(profileSettingsTitle("api-tokens")).toBe("Personal Access Tokens · Profile settings");
  });
});
