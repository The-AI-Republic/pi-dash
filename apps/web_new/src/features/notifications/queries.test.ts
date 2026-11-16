// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";
import { notificationKeys } from "./api/keys.js";
import {
  NOTIFICATIONS_FIRST_CURSOR,
  NOTIFICATIONS_PAGE_SIZE,
  nextNotificationCursor,
  notificationPagesQueryOptions,
  notificationsListQueryOptions,
  unreadNotificationsQueryOptions,
} from "./api/queries.js";
import { notificationStreamQuery, notificationTabQuery } from "./lib/stream.js";

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

  it("keys set type and read filters as their own entries", () => {
    expect(notificationKeys.list("acme", { type: "assigned", snoozed: false, archived: false, read: true })).toEqual([
      "ws",
      "acme",
      "notifications",
      "list",
      "assigned",
      false,
      false,
      true,
      false,
    ]);
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

  it("pages from the initial cursor through each opaque continuation", async () => {
    const first = { ...pageEnvelope(), next_page_results: true, next_cursor: "300:300:1" };
    const second = { ...pageEnvelope(), next_page_results: false, next_cursor: "" };
    const getJson = vi.fn(async (_path: string, options: { query: { cursor: string } }) =>
      options.query.cursor === NOTIFICATIONS_FIRST_CURSOR ? first : second
    );
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = notificationPagesQueryOptions(fakeClient, "acme", notificationTabQuery("all"));
    expect(options.queryKey).toEqual([...notificationKeys.list("acme", notificationTabQuery("all")), "pages"]);
    await expect(options.queryFn?.({ pageParam: NOTIFICATIONS_FIRST_CURSOR } as never)).resolves.toEqual(first);
    await expect(options.queryFn?.({ pageParam: first.next_cursor } as never)).resolves.toEqual(second);
    expect(nextNotificationCursor(first)).toBe("300:300:1");
    expect(nextNotificationCursor(second)).toBeUndefined();
    expect(nextNotificationCursor({ ...second, next_page_results: true })).toBeUndefined();
  });

  it("keeps the cursor stream disabled without a workspace", () => {
    const fakeClient = { getJson: vi.fn(), parse: vi.fn() } as never;
    expect(notificationPagesQueryOptions(fakeClient, "", notificationTabQuery("all")).enabled).toBe(false);
  });

  it("re-reads on remount but never on focus or reconnect (NTF-028)", () => {
    const fakeClient = {
      getJson: () => Promise.resolve({}),
      parse: (_schema: unknown, data: unknown) => data,
    } as never;
    const query = notificationTabQuery("all");
    const list = notificationsListQueryOptions(fakeClient, "acme", query);
    const pages = notificationPagesQueryOptions(fakeClient, "acme", query);
    const unread = unreadNotificationsQueryOptions(fakeClient, "acme");
    expect(list.refetchOnMount).toBe("always");
    expect(list.refetchOnWindowFocus).toBe(false);
    expect(list.refetchOnReconnect).toBe(false);
    expect(pages.refetchOnWindowFocus).toBe(false);
    expect(pages.refetchOnReconnect).toBe(false);
    expect(unread.refetchOnMount).toBe("always");
    expect(unread.refetchOnWindowFocus).toBe(false);
    expect(unread.refetchOnReconnect).toBe(false);
  });

  it("sends the selected origins and the unread flag with the list query", async () => {
    const getJson = vi.fn(async () => ({ results: [] }));
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = notificationsListQueryOptions(
      fakeClient,
      "acme",
      notificationStreamQuery("all", ["assigned", "created"], { unreadOnly: true, archived: false, snoozed: false })
    );
    await options.queryFn?.({} as never);
    expect(getJson).toHaveBeenCalledWith("/api/workspaces/acme/users/notifications", {
      query: {
        type: "assigned,created",
        snoozed: "false",
        archived: "false",
        read: "false",
        per_page: NOTIFICATIONS_PAGE_SIZE,
        cursor: NOTIFICATIONS_FIRST_CURSOR,
      },
    });
  });
});

function pageEnvelope() {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: 0,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: 0,
    total_results: 0,
    results: [],
  };
}
