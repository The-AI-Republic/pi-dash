// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import { isAccountSettingsPathname, isNotificationsPath } from "./sidebarAutohide.js";

describe("isNotificationsPath", () => {
  it("matches the workspace notifications route only", () => {
    expect(isNotificationsPath("/acme/notifications/")).toBe(true);
    expect(isNotificationsPath("/acme/notifications")).toBe(true);
    expect(isNotificationsPath("/acme/")).toBe(false);
    expect(isNotificationsPath("/acme/projects/")).toBe(false);
  });

  it("does not treat an account tab named notifications as the inbox", () => {
    // The account notifications tab lives under `settings/account`; the
    // segment further along must not unmount the workspace chrome.
    expect(isNotificationsPath("/acme/settings/account/notifications/")).toBe(false);
    expect(isNotificationsPath("/acme/settings/account/notifications")).toBe(false);
    expect(isNotificationsPath("/notifications/")).toBe(false);
  });
});

describe("isAccountSettingsPathname", () => {
  it("matches every account tab of a workspace", () => {
    expect(isAccountSettingsPathname("/acme/settings/account/general/")).toBe(true);
    expect(isAccountSettingsPathname("/acme/settings/account/notifications/")).toBe(true);
    expect(isAccountSettingsPathname("/acme/settings/account")).toBe(true);
  });

  it("rejects workspace settings, project settings and other pages", () => {
    expect(isAccountSettingsPathname("/acme/settings/members/")).toBe(false);
    expect(isAccountSettingsPathname("/acme/settings/projects/p1/")).toBe(false);
    expect(isAccountSettingsPathname("/acme/")).toBe(false);
    expect(isAccountSettingsPathname("/settings/profile/general/")).toBe(false);
  });
});
