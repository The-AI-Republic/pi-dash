// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The stream decoder drives where a card write lands, so it has its own
// suite: every key shape the inbox caches round-trips, and keys from other
// roots or workspaces are refused instead of misread.
import { describe, expect, it } from "vitest";

import { notificationKeys } from "./keys.js";
import { notificationListQueryFromKey } from "./streamQuery.js";

const WS = "acme";

describe("notificationListQueryFromKey", () => {
  it("rebuilds each stream query from its key", () => {
    for (const query of [
      { snoozed: false, archived: false },
      { snoozed: false, archived: true },
      { snoozed: true, archived: false },
      { snoozed: false, archived: false, mentioned: true },
      { snoozed: false, archived: false, read: false },
      { snoozed: false, archived: false, type: "assigned,created" },
    ]) {
      expect(notificationListQueryFromKey(WS, notificationKeys.list(WS, query))).toEqual(query);
    }
  });

  it("rebuilds the cursor stream's query from its pages key", () => {
    expect(
      notificationListQueryFromKey(WS, notificationKeys.pages(WS, { snoozed: false, archived: false, type: "comment" }))
    ).toEqual({ snoozed: false, archived: false, type: "comment" });
  });

  it("rejects keys that are not list keys of this workspace", () => {
    expect(notificationListQueryFromKey(WS, notificationKeys.unread(WS))).toBeNull();
    expect(
      notificationListQueryFromKey("other", notificationKeys.list(WS, { snoozed: false, archived: false }))
    ).toBeNull();
    expect(
      notificationListQueryFromKey(WS, [...notificationKeys.list(WS, { snoozed: false, archived: false }), true])
    ).toBeNull();
  });
});
