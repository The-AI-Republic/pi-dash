// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";
import { notificationKeys } from "./api/keys.js";
import {
  NOTIFICATIONS_FIRST_CURSOR,
  NOTIFICATIONS_PAGE_SIZE,
  notificationsListQueryOptions,
  unreadNotificationsQueryOptions,
} from "./api/queries.js";
import { notificationTabQuery } from "./lib/stream.js";

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

describe("notifications list query", () => {
  it("keys each stream by its full query", () => {
    expect(notificationKeys.list("acme", notificationTabQuery("all"))).toEqual([
      "ws",
      "acme",
      "notifications",
      "list",
      null,
      false,
      false,
      null,
      false,
    ]);
    expect(notificationKeys.list("acme", notificationTabQuery("mentions"))).toEqual([
      "ws",
      "acme",
      "notifications",
      "list",
      null,
      false,
      false,
      null,
      true,
    ]);
  });

  it("keys mentioned-false with the absent flag (same stream server-side)", () => {
    expect(notificationKeys.list("acme", { mentioned: false })).toEqual(notificationKeys.list("acme", {}));
  });

  it("fetches the first page with the fixed page size and initial cursor", async () => {
    const page = { results: [], next_page_results: false };
    const getJson = vi.fn(async () => page);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const query = notificationTabQuery("all");
    const options = notificationsListQueryOptions(fakeClient, "acme", query);
    expect(options.queryKey).toEqual(notificationKeys.list("acme", query));
    expect(options.enabled).toBe(true);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(page);
    expect(getJson).toHaveBeenCalledWith("/api/workspaces/acme/users/notifications", {
      query: {
        snoozed: "false",
        archived: "false",
        per_page: NOTIFICATIONS_PAGE_SIZE,
        cursor: NOTIFICATIONS_FIRST_CURSOR,
      },
    });
    expect(NOTIFICATIONS_PAGE_SIZE).toBe(300);
    expect(NOTIFICATIONS_FIRST_CURSOR).toBe("300:0:0");
  });

  it("sends the mentioned flag only on the mentions stream", async () => {
    const getJson = vi.fn(async () => ({ results: [] }));
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = notificationsListQueryOptions(fakeClient, "acme", notificationTabQuery("mentions"));
    await options.queryFn?.({} as never);
    expect(getJson).toHaveBeenCalledWith("/api/workspaces/acme/users/notifications", {
      query: {
        snoozed: "false",
        archived: "false",
        mentioned: "true",
        per_page: NOTIFICATIONS_PAGE_SIZE,
        cursor: NOTIFICATIONS_FIRST_CURSOR,
      },
    });
  });

  it("stays disabled without a workspace", () => {
    const fakeClient = {
      getJson: () => Promise.resolve({}),
      parse: (_schema: unknown, data: unknown) => data,
    } as never;
    expect(notificationsListQueryOptions(fakeClient, "", notificationTabQuery("all")).enabled).toBe(false);
  });
});
