// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { NotificationItem, NotificationListResponse, TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notificationTabQuery } from "../lib/stream.js";
import { notificationKeys } from "./keys.js";
import {
  applyReadToNotificationCache,
  applyReadToTotals,
  applySuccessfulRead,
  useMarkNotificationRead,
} from "./read.js";

const ID = "523e4567-e89b-12d3-a456-426614174004";
const WORKSPACE = "323e4567-e89b-12d3-a456-426614174002";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const READ_AT = "2026-10-17T12:30:00Z";

function item(overrides: Partial<NotificationItem> = {}): NotificationItem {
  return {
    id: ID,
    workspace: WORKSPACE,
    project: PROJECT,
    data: { issue: { id: ISSUE } },
    entity_identifier: ISSUE,
    entity_name: "issue",
    title: "Update",
    message: null,
    message_html: "",
    message_stripped: null,
    sender: "in_app:issue_activities:default",
    triggered_by: null,
    triggered_by_details: null,
    receiver: WORKSPACE,
    read_at: null,
    snoozed_till: null,
    archived_at: null,
    is_inbox_issue: false,
    is_intake_issue: false,
    is_mentioned_notification: true,
    created_at: "2026-10-17T12:00:00Z",
    updated_at: "2026-10-17T12:00:00Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

function page(rows: NotificationItem[]): NotificationListResponse {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: rows.length,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: rows.length,
    total_results: rows.length,
    results: rows,
  };
}

function response(url: string, body: unknown): TransportResponse {
  return {
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  } as TransportResponse;
}

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: React.ReactNode }> {
  return function Wrapper({ children }: { children?: React.ReactNode }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

afterEach(() => {
  resetAppClient();
});

describe("read cache helpers", () => {
  it("patches both first-page and cursor-page shapes without replacing stream fields", () => {
    const other = item({ id: "623e4567-e89b-12d3-a456-426614174005" });
    const firstPage = page([item(), other]);
    const patchedPage = applyReadToNotificationCache(firstPage, ID, READ_AT) as NotificationListResponse;
    expect(patchedPage.results[0]).toMatchObject({ id: ID, read_at: READ_AT, is_mentioned_notification: true });
    expect(patchedPage.results[1]).toBe(other);

    const infinite = { pages: [firstPage, page([])], pageParams: ["300:0:0", "300:300:1"] };
    const patchedInfinite = applyReadToNotificationCache(infinite, ID, READ_AT) as typeof infinite;
    expect(patchedInfinite.pages[0]?.results[0]?.read_at).toBe(READ_AT);
    expect(patchedInfinite.pageParams).toEqual(infinite.pageParams);
  });

  it("leaves absent and unrelated cache shapes unchanged", () => {
    expect(applyReadToNotificationCache(undefined, ID, READ_AT)).toBeUndefined();
    const malformed = { pages: [page([]), "not-a-page"] };
    expect(applyReadToNotificationCache(malformed, ID, READ_AT)).toBe(malformed);
    expect(applyReadToNotificationCache("other", ID, READ_AT)).toBe("other");
  });

  it("decrements overall and only the matching mention total, clamped at zero", () => {
    expect(applyReadToTotals(undefined, true)).toBeUndefined();
    expect(
      applyReadToTotals({ total_unread_notifications_count: 2, mention_unread_notifications_count: 1 }, true)
    ).toEqual({ total_unread_notifications_count: 1, mention_unread_notifications_count: 0 });
    expect(
      applyReadToTotals({ total_unread_notifications_count: 0, mention_unread_notifications_count: 2 }, false)
    ).toEqual({ total_unread_notifications_count: 0, mention_unread_notifications_count: 2 });
  });

  it("does not patch when the server does not confirm a read timestamp", () => {
    const queryClient = new QueryClient();
    const cached = page([item()]);
    queryClient.setQueryData(notificationKeys.list("acme", notificationTabQuery("all")), cached);
    applySuccessfulRead(queryClient, "acme", item(), true);
    expect(queryClient.getQueryData(notificationKeys.list("acme", notificationTabQuery("all")))).toBe(cached);
  });
});

describe("useMarkNotificationRead", () => {
  it("posts one read and updates every stream cache plus the totals", async () => {
    const calls: Array<{ url: string; method: string }> = [];
    createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
      calls.push({ url, method: init.method ?? "GET" });
      if (url.endsWith("/auth/get-csrf-token/")) return response(url, { csrf_token: "csrf" });
      return response(url, item({ read_at: READ_AT }));
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const allKey = notificationKeys.pages("acme", notificationTabQuery("all"));
    const mentionsKey = notificationKeys.pages("acme", notificationTabQuery("mentions"));
    queryClient.setQueryData(allKey, { pages: [page([item()])], pageParams: ["300:0:0"] });
    queryClient.setQueryData(mentionsKey, { pages: [page([item()])], pageParams: ["300:0:0"] });
    queryClient.setQueryData(notificationKeys.unread("acme"), {
      total_unread_notifications_count: 3,
      mention_unread_notifications_count: 2,
    });
    const { result } = renderHook(() => useMarkNotificationRead("acme"), { wrapper: wrapper(queryClient) });

    await act(async () => {
      await result.current.mutateAsync({ id: ID, mentioned: true });
    });

    expect(calls).toContainEqual({
      url: `https://api.test/api/workspaces/acme/users/notifications/${ID}/read/`,
      method: "POST",
    });
    expect(
      (queryClient.getQueryData(allKey) as { pages: NotificationListResponse[] }).pages[0]?.results[0]?.read_at
    ).toBe(READ_AT);
    expect(
      (queryClient.getQueryData(mentionsKey) as { pages: NotificationListResponse[] }).pages[0]?.results[0]?.read_at
    ).toBe(READ_AT);
    expect(queryClient.getQueryData(notificationKeys.unread("acme"))).toEqual({
      total_unread_notifications_count: 2,
      mention_unread_notifications_count: 1,
    });
  });
});
