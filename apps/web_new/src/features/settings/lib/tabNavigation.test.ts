// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { PROFILE_SETTINGS_TABS, profileSettingsGroups, profileSettingsTitle, findProfileSettingsTab } from "./tabs.js";
import {
  DEFAULT_PROFILE_SETTINGS_TAB,
  isProfileSettingsTab,
  profileSettingsHref,
  PROFILE_SETTINGS_TAB_IDS,
  queryWithoutMark,
  resolveProfileSettingsTab,
} from "./tabNavigation.js";

describe("profile settings addresses", () => {
  it("lists the nine tab ids in display order", () => {
    expect(PROFILE_SETTINGS_TAB_IDS).toEqual([
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
    expect(DEFAULT_PROFILE_SETTINGS_TAB).toBe("general");
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
  });

  it("builds tab links with a trailing slash and keeps a query verbatim", () => {
    expect(profileSettingsHref("general")).toBe("/settings/profile/general/");
    expect(profileSettingsHref("security", "?keep=2")).toBe("/settings/profile/security/?keep=2");
    expect(profileSettingsHref("security", "keep=2")).toBe("/settings/profile/security/?keep=2");
    expect(profileSettingsHref("security", "?")).toBe("/settings/profile/security/");
    expect(queryWithoutMark("?a=1")).toBe("a=1");
    expect(queryWithoutMark("a=1")).toBe("a=1");
  });
});

describe("profile settings sidebar model", () => {
  it("groups every id under its label", () => {
    expect(PROFILE_SETTINGS_TABS.map((tab) => tab.id)).toEqual([...PROFILE_SETTINGS_TAB_IDS]);
    expect(profileSettingsGroups().map((group) => group.label)).toEqual(["Your Profile", "Developer"]);
    expect(profileSettingsGroups().map((group) => group.tabs.map((tab) => tab.label))).toEqual([
      ["Profile", "Preferences", "AI Assistant", "Auto Project Management", "Notifications", "Security", "Activity"],
      ["Integrations", "Personal Access Tokens"],
    ]);
  });

  it("finds a tab by id and titles the document", () => {
    expect(findProfileSettingsTab("notifications").label).toBe("Notifications");
    expect(profileSettingsTitle("api-tokens")).toBe("Personal Access Tokens · Profile settings");
  });
});
