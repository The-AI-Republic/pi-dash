// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Card writes (parity/specs/notifications.md §9, NTF-018/019/020): the
// read and archive toggles hit the per-item paths, move every cached
// stream and the unread totals, and toast only on success; snooze writes
// the resume moment through the per-item update. A failed write restores
// the exact caches it touched and stays silent.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { NotificationItem, NotificationListResponse, TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notificationKeys, notificationListQueryFromKey } from "./keys.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import {
  MARKED_ARCHIVED_TITLE,
  MARKED_READ_TITLE,
  MARKED_UNREAD_TITLE,
  SNOOZED_TITLE,
  UNARCHIVED_TITLE,
  UNSNOOZED_TITLE,
  applyRowToPage,
  mergeWrittenRow,
  rowBelongsToStream,
  useSnoozeNotification,
  useToggleNotificationArchive,
  useToggleNotificationRead,
  type StreamFlags,
} from "./mutations.js";

const WS = "acme";
const ID = "523e4567-e89b-12d3-a456-426614174004";
const OTHER = "623e4567-e89b-12d3-a456-426614174005";
const FLAGS: StreamFlags = { is_inbox_issue: true, is_intake_issue: false, is_mentioned_notification: false };

function row(overrides: Partial<NotificationItem> = {}): NotificationItem {
  return {
    id: ID,
    workspace: "323e4567-e89b-12d3-a456-426614174002",
    project: "223e4567-e89b-12d3-a456-426614174001",
    data: { issue: { name: "Ship it" }, issue_activity: { field: "comment", verb: "created" } },
    entity_identifier: "123e4567-e89b-12d3-a456-426614174000",
    entity_name: "issue",
    title: "",
    message: null,
    message_html: "",
    message_stripped: null,
    sender: "in_app:issue_activities:default",
    triggered_by: "423e4567-e89b-12d3-a456-426614174003",
    triggered_by_details: null,
    receiver: "723e4567-e89b-12d3-a456-426614174006",
    read_at: null,
    snoozed_till: null,
    archived_at: null,
    is_mentioned_notification: false,
    created_at: "2026-10-16T12:00:00Z",
    updated_at: "2026-10-16T12:00:00Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

function page(results: NotificationItem[]): NotificationListResponse {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: results.length,
    next_cursor: "300:0:0",
    prev_cursor: "300:-1:0",
    next_page_results: false,
    prev_page_results: false,
    count: results.length,
    total_results: results.length,
    results,
  };
}

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return {
    status,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  } as TransportResponse;
}

interface StubOptions {
  echo?: (sent: { method: string; body: string }) => NotificationItem;
  failWrites?: boolean;
}

function stubApi(options: StubOptions = {}) {
  const sent: Array<{ method: string; url: string; body: string }> = [];
  const itemPath = `/api/workspaces/${WS}/users/notifications/${ID}/`;
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    const method = init.method ?? "GET";
    const body = init.body ?? "{}";
    if (!url.includes(itemPath) && !url.startsWith(itemPath)) throw new Error(`unexpected fetch ${url}`);
    sent.push({ method, url, body });
    if (options.failWrites === true) return jsonResponse(url, 500, { detail: "parity card failure" });
    // The echo is the bare instance: the list annotations are absent.
    if (options.echo !== undefined) return jsonResponse(url, 200, options.echo({ method, body }));
    if (url.endsWith("/read/"))
      return jsonResponse(url, 200, row({ read_at: method === "POST" ? "2026-10-17T10:00:00Z" : null }));
    if (url.endsWith("/archive/")) {
      return jsonResponse(url, 200, row({ archived_at: method === "POST" ? "2026-10-17T10:00:00Z" : null }));
    }
    const parsed = JSON.parse(body) as { snoozed_till: string | null };
    return jsonResponse(url, 200, row({ snoozed_till: parsed.snoozed_till }));
  }) as never);
  return { sent };
}

function wrapper(client: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={client}>{children as never}</QueryClientProvider>;
  };
}

function seededClient(
  options: {
    unread?: number;
    archivedPage?: boolean;
    snoozedPage?: boolean;
    archivedSeed?: NotificationItem[];
    snoozedSeed?: NotificationItem[];
  } = {}
): QueryClient {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const streamQuery = { snoozed: false, archived: false };
  const streams: Array<{ query: typeof streamQuery; rows: NotificationItem[] }> = [
    { query: streamQuery, rows: [row(), row({ id: OTHER })] },
  ];
  const archivedSeed = options.archivedSeed ?? [];
  const snoozedSeed = options.snoozedSeed ?? [];
  if (options.archivedPage ?? true) {
    streams.push({ query: { snoozed: false, archived: true }, rows: archivedSeed });
  }
  if (options.snoozedPage ?? true) {
    streams.push({ query: { snoozed: true, archived: false }, rows: snoozedSeed });
  }
  for (const { query, rows } of streams) {
    client.setQueryData(notificationKeys.list(WS, query), page(rows));
  }
  client.setQueryData(notificationKeys.unread(WS), {
    total_unread_notifications_count: options.unread ?? 2,
    mention_unread_notifications_count: 1,
  });
  return client;
}

function streamRows(client: QueryClient, query: { snoozed: boolean; archived: boolean }): string[] {
  const data = client.getQueryData<NotificationListResponse>(notificationKeys.list(WS, query));
  return (data?.results ?? []).map((entry) => entry.id);
}

beforeEach(() => {
  resetAppClient();
});

afterEach(() => {
  vi.clearAllMocks();
});

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

describe("rowBelongsToStream", () => {
  it("puts archived cards only in archived mode", () => {
    const archived = row({ archived_at: "2026-10-17T10:00:00Z" });
    expect(rowBelongsToStream(archived, { snoozed: false, archived: true })).toBe(true);
    expect(rowBelongsToStream(archived, { snoozed: false, archived: false })).toBe(false);
    expect(rowBelongsToStream(archived, { snoozed: true, archived: false })).toBe(false);
  });

  it("puts snoozed cards only in snoozed mode", () => {
    const snoozed = row({ snoozed_till: "2026-10-19T17:30:00Z" });
    expect(rowBelongsToStream(snoozed, { snoozed: true, archived: false })).toBe(true);
    expect(rowBelongsToStream(snoozed, { snoozed: false, archived: true })).toBe(false);
    expect(rowBelongsToStream(snoozed, { snoozed: false, archived: false })).toBe(false);
  });

  it("keeps plain cards in the default stream only", () => {
    expect(rowBelongsToStream(row(), { snoozed: false, archived: false })).toBe(true);
    expect(rowBelongsToStream(row(), { snoozed: true, archived: false })).toBe(false);
    expect(rowBelongsToStream(row(), { snoozed: false, archived: true })).toBe(false);
  });

  it("lists everything in a stream without mode flags", () => {
    const archived = row({ archived_at: "2026-10-17T10:00:00Z" });
    expect(rowBelongsToStream(archived, {})).toBe(true);
    expect(rowBelongsToStream(row({ snoozed_till: "2026-10-19T17:30:00Z" }), {})).toBe(true);
  });
});

describe("applyRowToPage", () => {
  it("replaces a listed row in place", () => {
    const next = applyRowToPage(
      page([row(), row({ id: OTHER })]),
      { snoozed: false, archived: false },
      row({ read_at: "now" })
    );
    expect(next?.results.map((entry) => entry.id)).toEqual([ID, OTHER]);
    expect(next?.results[0]?.read_at).toBe("now");
  });

  it("joins a row the stream now includes at the top", () => {
    const next = applyRowToPage(page([]), { snoozed: false, archived: true }, row({ archived_at: "now" }));
    expect(next?.results.map((entry) => entry.id)).toEqual([ID]);
  });

  it("drops a row the stream no longer includes", () => {
    const next = applyRowToPage(page([row()]), { snoozed: false, archived: true }, row());
    expect(next?.results).toEqual([]);
  });

  it("leaves a page that neither lists nor admits the row untouched", () => {
    const original = page([row({ id: OTHER })]);
    expect(applyRowToPage(original, { snoozed: false, archived: true }, row())).toBe(original);
  });

  it("passes an absent page through", () => {
    expect(applyRowToPage(undefined, {}, row())).toBeUndefined();
  });
});

describe("mergeWrittenRow", () => {
  it("keeps the listed stream annotations the echo drops", () => {
    const merged = mergeWrittenRow(
      row({ is_inbox_issue: true, is_intake_issue: false, is_mentioned_notification: false }),
      row({
        read_at: "now",
        is_inbox_issue: undefined,
        is_intake_issue: undefined,
        is_mentioned_notification: undefined,
      })
    );
    expect(merged.read_at).toBe("now");
    expect(merged.is_inbox_issue).toBe(true);
    expect(merged.is_intake_issue).toBe(false);
    expect(merged.is_mentioned_notification).toBe(false);
  });

  it("keeps absent annotations absent", () => {
    const merged = mergeWrittenRow(row(), row({ archived_at: "now" }));
    expect(merged.is_inbox_issue).toBeUndefined();
    expect(merged.is_intake_issue).toBeUndefined();
    expect(merged.is_mentioned_notification).toBe(false);
    expect(merged.archived_at).toBe("now");
  });

  it("keeps the written mention flag when the listed row has none", () => {
    const merged = mergeWrittenRow(
      row({ is_mentioned_notification: undefined }),
      row({ is_mentioned_notification: true })
    );
    expect(merged.is_mentioned_notification).toBe(true);
  });
});

describe("useToggleNotificationRead", () => {
  it("marks a card read with POST, shifts the totals and toasts", async () => {
    const { sent } = stubApi();
    const client = seededClient({ unread: 2 });
    const { result } = renderHook(() => useToggleNotificationRead(WS), { wrapper: wrapper(client) });

    await result.current.mutateAsync({ id: ID, unread: true, flags: FLAGS });

    expect(sent).toHaveLength(1);
    expect(sent[0]?.method).toBe("POST");
    expect(sent[0]?.url).toContain(`/users/notifications/${ID}/read/`);
    const cards = client.getQueryData<NotificationListResponse>(
      notificationKeys.list(WS, { snoozed: false, archived: false })
    );
    expect(cards?.results.find((entry) => entry.id === ID)?.read_at).not.toBeNull();
    expect(cards?.results.find((entry) => entry.id === ID)?.is_mentioned_notification).toBe(false);
    expect(
      client.getQueryData<{ total_unread_notifications_count: number }>(notificationKeys.unread(WS))
        ?.total_unread_notifications_count
    ).toBe(1);
    expect(notifySuccess).toHaveBeenCalledWith(MARKED_READ_TITLE);
  });

  it("marks a card unread with DELETE and raises the totals", async () => {
    const { sent } = stubApi();
    const client = seededClient({ unread: 1 });
    client.setQueryData(
      notificationKeys.list(WS, { snoozed: false, archived: false }),
      page([row({ read_at: "2026-10-16T13:00:00Z" })])
    );
    const { result } = renderHook(() => useToggleNotificationRead(WS), { wrapper: wrapper(client) });

    await result.current.mutateAsync({ id: ID, unread: false, flags: FLAGS });

    expect(sent[0]?.method).toBe("DELETE");
    expect(
      client.getQueryData<{ total_unread_notifications_count: number }>(notificationKeys.unread(WS))
        ?.total_unread_notifications_count
    ).toBe(2);
    expect(notifySuccess).toHaveBeenCalledWith(MARKED_UNREAD_TITLE);
  });

  it("rolls the card and totals back on failure without a toast", async () => {
    stubApi({ failWrites: true });
    const client = seededClient({ unread: 3 });
    const before = client.getQueryData(notificationKeys.list(WS, { snoozed: false, archived: false }));
    const { result } = renderHook(() => useToggleNotificationRead(WS), { wrapper: wrapper(client) });

    await expect(result.current.mutateAsync({ id: ID, unread: true, flags: FLAGS })).rejects.toThrow();

    expect(client.getQueryData(notificationKeys.list(WS, { snoozed: false, archived: false }))).toEqual(before);
    expect(
      client.getQueryData<{ total_unread_notifications_count: number }>(notificationKeys.unread(WS))
        ?.total_unread_notifications_count
    ).toBe(3);
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("retries cleanly after a failed write", async () => {
    let calls = 0;
    createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
      if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
      calls += 1;
      if (calls === 1) return jsonResponse(url, 500, { detail: "transient" });
      void init;
      return jsonResponse(url, 200, row({ read_at: "2026-10-17T10:00:00Z" }));
    }) as never);
    const client = seededClient({ unread: 1 });
    const { result } = renderHook(() => useToggleNotificationRead(WS), { wrapper: wrapper(client) });

    await expect(result.current.mutateAsync({ id: ID, unread: true, flags: FLAGS })).rejects.toThrow();
    await result.current.mutateAsync({ id: ID, unread: true, flags: FLAGS });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(MARKED_READ_TITLE));
  });

  it("skips streams that do not list the card", async () => {
    stubApi();
    const client = seededClient({ unread: 1 });
    const mentions = page([row({ id: OTHER })]);
    client.setQueryData(notificationKeys.list(WS, { snoozed: false, archived: false, mentioned: true }), mentions);
    client.setQueryData(notificationKeys.list(WS, { snoozed: false, archived: false }), page([row({ id: OTHER })]));
    const { result } = renderHook(() => useToggleNotificationRead(WS), { wrapper: wrapper(client) });

    await result.current.mutateAsync({ id: ID, unread: true, flags: FLAGS });

    expect(
      client.getQueryData(notificationKeys.list(WS, { snoozed: false, archived: false, mentioned: true }))
    ).toEqual(mentions);
  });
});

describe("useToggleNotificationArchive", () => {
  it("archives with POST: the card leaves the default stream and enters archived mode", async () => {
    const { sent } = stubApi();
    const client = seededClient();
    const { result } = renderHook(() => useToggleNotificationArchive(WS), { wrapper: wrapper(client) });

    await result.current.mutateAsync({ id: ID, archived: true, flags: FLAGS });

    expect(sent[0]?.method).toBe("POST");
    expect(sent[0]?.url).toContain(`/users/notifications/${ID}/archive/`);
    expect(streamRows(client, { snoozed: false, archived: false })).toEqual([OTHER]);
    expect(streamRows(client, { snoozed: false, archived: true })).toEqual([ID]);
    expect(notifySuccess).toHaveBeenCalledWith(MARKED_ARCHIVED_TITLE);
  });

  it("unarchives with DELETE: the card leaves archived mode and returns", async () => {
    const { sent } = stubApi();
    const client = seededClient();
    client.setQueryData(
      notificationKeys.list(WS, { snoozed: false, archived: true }),
      page([row({ archived_at: "2026-10-16T13:00:00Z" })])
    );
    client.setQueryData(notificationKeys.list(WS, { snoozed: false, archived: false }), page([row({ id: OTHER })]));
    const { result } = renderHook(() => useToggleNotificationArchive(WS), { wrapper: wrapper(client) });

    await result.current.mutateAsync({ id: ID, archived: false, flags: FLAGS });

    expect(sent[0]?.method).toBe("DELETE");
    expect(streamRows(client, { snoozed: false, archived: true })).toEqual([]);
    expect(streamRows(client, { snoozed: false, archived: false })).toEqual([ID, OTHER]);
    expect(notifySuccess).toHaveBeenCalledWith(UNARCHIVED_TITLE);
  });

  it("rolls back the streams on failure", async () => {
    stubApi({ failWrites: true });
    const client = seededClient();
    const before = client.getQueryData(notificationKeys.list(WS, { snoozed: false, archived: false }));
    const { result } = renderHook(() => useToggleNotificationArchive(WS), { wrapper: wrapper(client) });

    await expect(result.current.mutateAsync({ id: ID, archived: true, flags: FLAGS })).rejects.toThrow();

    expect(client.getQueryData(notificationKeys.list(WS, { snoozed: false, archived: false }))).toEqual(before);
    expect(notifySuccess).not.toHaveBeenCalled();
  });
});

describe("useSnoozeNotification", () => {
  it("snoozes with the resume moment: leaves the stream, enters snoozed mode", async () => {
    const { sent } = stubApi();
    const client = seededClient();
    const { result } = renderHook(() => useSnoozeNotification(WS), { wrapper: wrapper(client) });
    const resume = "2026-10-19T17:30:00.000Z";

    await result.current.mutateAsync({ id: ID, snoozedTill: resume, flags: FLAGS });

    expect(sent[0]?.method).toBe("PATCH");
    expect(sent[0]?.url).toContain(`/users/notifications/${ID}/`);
    expect(JSON.parse(sent[0]?.body ?? "{}")).toEqual({ snoozed_till: resume });
    expect(streamRows(client, { snoozed: false, archived: false })).toEqual([OTHER]);
    expect(streamRows(client, { snoozed: true, archived: false })).toEqual([ID]);
    expect(notifySuccess).toHaveBeenCalledWith(SNOOZED_TITLE);
  });

  it("unsnoozes with a null resume: leaves snoozed mode and returns", async () => {
    const { sent } = stubApi();
    const client = seededClient();
    client.setQueryData(
      notificationKeys.list(WS, { snoozed: true, archived: false }),
      page([row({ snoozed_till: "2026-10-19T17:30:00Z" })])
    );
    client.setQueryData(notificationKeys.list(WS, { snoozed: false, archived: false }), page([row({ id: OTHER })]));
    const { result } = renderHook(() => useSnoozeNotification(WS), { wrapper: wrapper(client) });

    await result.current.mutateAsync({ id: ID, snoozedTill: null, flags: FLAGS });

    expect(JSON.parse(sent[0]?.body ?? "{}")).toEqual({ snoozed_till: null });
    expect(streamRows(client, { snoozed: true, archived: false })).toEqual([]);
    expect(streamRows(client, { snoozed: false, archived: false })).toEqual([ID, OTHER]);
    expect(notifySuccess).toHaveBeenCalledWith(UNSNOOZED_TITLE);
  });

  it("rolls everything back on a failed snooze", async () => {
    stubApi({ failWrites: true });
    const client = seededClient({ unread: 2 });
    const before = client.getQueryData(notificationKeys.list(WS, { snoozed: false, archived: false }));
    const beforeUnread = client.getQueryData(notificationKeys.unread(WS));
    const { result } = renderHook(() => useSnoozeNotification(WS), { wrapper: wrapper(client) });

    await expect(
      result.current.mutateAsync({ id: ID, snoozedTill: "2026-10-19T17:30:00.000Z", flags: FLAGS })
    ).rejects.toThrow();

    expect(client.getQueryData(notificationKeys.list(WS, { snoozed: false, archived: false }))).toEqual(before);
    expect(client.getQueryData(notificationKeys.unread(WS))).toEqual(beforeUnread);
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("settles the server's resume stamp into the cached card", async () => {
    stubApi({ echo: ({ body }) => row({ snoozed_till: (JSON.parse(body) as { snoozed_till: string }).snoozed_till }) });
    const client = seededClient();
    const { result } = renderHook(() => useSnoozeNotification(WS), { wrapper: wrapper(client) });

    await result.current.mutateAsync({ id: ID, snoozedTill: "2026-10-19T17:30:00.000Z", flags: FLAGS });

    const snoozed = client.getQueryData<NotificationListResponse>(
      notificationKeys.list(WS, { snoozed: true, archived: false })
    );
    expect(snoozed?.results[0]?.snoozed_till).toBe("2026-10-19T17:30:00.000Z");
    expect(snoozed?.results[0]?.is_mentioned_notification).toBe(false);
  });
});
