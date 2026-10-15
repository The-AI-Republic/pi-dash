// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";
import { notificationKeys } from "./api/keys.js";
import { unreadNotificationsQueryOptions } from "./api/queries.js";

describe("unread notifications query", () => {
  it("keys the count by workspace", () => {
    expect(notificationKeys.unread("acme")).toEqual(["ws", "acme", "notifications", "unread"]);
  });

  it("fetches the unread count for the workspace", async () => {
    const unread = { total_unread_notifications_count: 3, mention_unread_notifications_count: 1 };
    const getJson = vi.fn(async () => unread);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = unreadNotificationsQueryOptions(fakeClient, "acme");
    expect(options.queryKey).toEqual(notificationKeys.unread("acme"));
    expect(options.enabled).toBe(true);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(unread);
    expect(getJson).toHaveBeenCalledWith("/api/workspaces/acme/users/notifications/unread/");
  });

  it("stays disabled without a workspace", () => {
    const fakeClient = {
      getJson: () => Promise.resolve({}),
      parse: (_schema: unknown, data: unknown) => data,
    } as never;
    expect(unreadNotificationsQueryOptions(fakeClient, "").enabled).toBe(false);
  });
});
