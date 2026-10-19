// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile tab table and role visibility (parity/specs/profile.md §1,
// PROF-002). Pure rules shared by the strip, the picker and the routes.
import { describe, expect, it } from "vitest";

import {
  PROFILE_DENIED_TABS,
  PROFILE_TABS,
  activeProfileTabKey,
  profileBase,
  profileTabBySegment,
  profileTabHref,
  visibleProfileTabs,
} from "./tabs.js";

const member = { canEdit: true };
const guest = { canEdit: false };

describe("profile tabs", () => {
  it("lists the five tabs in strip order with summary open to everyone", () => {
    expect(PROFILE_TABS.map((tab) => tab.label)).toEqual(["Summary", "Assigned", "Created", "Subscribed", "Activity"]);
    expect(PROFILE_TABS.filter((tab) => tab.everyone).map((tab) => tab.key)).toEqual(["summary"]);
    expect(PROFILE_DENIED_TABS).toEqual(["assigned", "created", "subscribed"]);
  });

  it("gives members and admins every tab and guests the summary alone", () => {
    expect(visibleProfileTabs(member).map((tab) => tab.key)).toEqual([
      "summary",
      "assigned",
      "created",
      "subscribed",
      "activity",
    ]);
    expect(visibleProfileTabs(guest).map((tab) => tab.key)).toEqual(["summary"]);
    expect(visibleProfileTabs(member)).not.toBe(PROFILE_TABS);
  });

  it("resolves route segments and answers null for unknown ids", () => {
    expect(profileTabBySegment("assigned")?.key).toBe("assigned");
    expect(profileTabBySegment("activity")?.key).toBe("activity");
    expect(profileTabBySegment("bogus")).toBeNull();
    expect(profileTabBySegment("")).toBeNull();
  });

  it("builds the base and tab addresses with the trailing slash", () => {
    expect(profileBase("acme", "u-1")).toBe("/acme/profile/u-1/");
    const tabs = new Map(PROFILE_TABS.map((tab) => [tab.key, tab]));
    expect(profileTabHref("acme", "u-1", tabs.get("summary")!)).toBe("/acme/profile/u-1/");
    expect(profileTabHref("acme", "u-1", tabs.get("assigned")!)).toBe("/acme/profile/u-1/assigned/");
    expect(profileTabHref("acme", "u-1", tabs.get("activity")!)).toBe("/acme/profile/u-1/activity/");
  });

  it("selects the current tab with or without a trailing slash", () => {
    expect(activeProfileTabKey("/acme/profile/u-1", "acme", "u-1")).toBe("summary");
    expect(activeProfileTabKey("/acme/profile/u-1/", "acme", "u-1")).toBe("summary");
    expect(activeProfileTabKey("/acme/profile/u-1/assigned/", "acme", "u-1")).toBe("assigned");
    expect(activeProfileTabKey("/acme/profile/u-1/created", "acme", "u-1")).toBe("created");
    expect(activeProfileTabKey("/acme/profile/u-1/subscribed", "acme", "u-1")).toBe("subscribed");
    expect(activeProfileTabKey("/acme/profile/u-1/activity/", "acme", "u-1")).toBe("activity");
  });

  it("answers null off the profile and for unknown segments", () => {
    expect(activeProfileTabKey("/acme/profile/u-2/assigned", "acme", "u-1")).toBeNull();
    expect(activeProfileTabKey("/acme/profile/u-1/bogus", "acme", "u-1")).toBeNull();
    expect(activeProfileTabKey("/acme/projects/u-1/assigned", "acme", "u-1")).toBeNull();
  });
});
