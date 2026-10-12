// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: notification unread count against live Django.
import { describe, expect, it } from "vitest";
import { getUnreadNotifications } from "../contracts/notifications.js";
import { CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";

describe.skipIf(!contractEnabled)("notifications contract", () => {
  it("answers total and mention unread counts", async () => {
    const client = await sharedSignedIn();
    const unread = await getUnreadNotifications(client, CONTRACT_WORKSPACE);
    expect(unread.total_unread_notifications_count).toBeGreaterThanOrEqual(0);
    expect(unread.mention_unread_notifications_count).toBeGreaterThanOrEqual(0);
  });
});
