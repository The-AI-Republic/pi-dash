// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { selectUnreadBadgeCount } from "./badgeCount.js";
import { NOTIFICATION_TABS, notificationTabQuery } from "./stream.js";

describe("notification tabs", () => {
  it("offers exactly the full stream and the mentions stream, in order", () => {
    expect([...NOTIFICATION_TABS]).toEqual(["all", "mentions"]);
  });

  it("queries the default stream with explicit exclusion flags", () => {
    expect(notificationTabQuery("all")).toEqual({ snoozed: false, archived: false });
  });

  it("queries the mentions stream with the mentioned-only flag", () => {
    expect(notificationTabQuery("mentions")).toEqual({ snoozed: false, archived: false, mentioned: true });
  });
});

describe("unread badge selection (NTF-003)", () => {
  it("prefers the mention total while any mention is unread", () => {
    expect(selectUnreadBadgeCount({ total_unread_notifications_count: 7, mention_unread_notifications_count: 2 })).toBe(
      2
    );
  });

  it("falls back to the overall total when no mention is unread", () => {
    expect(selectUnreadBadgeCount({ total_unread_notifications_count: 7, mention_unread_notifications_count: 0 })).toBe(
      7
    );
  });

  it("reads zero at zero and while the totals are unknown", () => {
    expect(selectUnreadBadgeCount({ total_unread_notifications_count: 0, mention_unread_notifications_count: 0 })).toBe(
      0
    );
    expect(selectUnreadBadgeCount(undefined)).toBe(0);
  });
});
