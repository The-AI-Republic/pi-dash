// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { NotificationItem, NotificationListResponse } from "@pidash/api-client";
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ROLE_ADMIN, ROLE_GUEST } from "../../../core/session/index.js";
import { sessionKeys } from "../../../core/session/queries.js";
import { notificationKeys } from "../api/keys.js";
import { notificationTabQuery } from "../lib/stream.js";
import { InboxView } from "./InboxView.js";

const ISSUE_ID = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const WORKSPACE_ID = "323e4567-e89b-12d3-a456-426614174002";
const ACTOR_ID = "423e4567-e89b-12d3-a456-426614174003";

function row(overrides: Partial<NotificationItem> = {}): NotificationItem {
  return {
    id: "523e4567-e89b-12d3-a456-426614174004",
    workspace: WORKSPACE_ID,
    project: PROJECT_ID,
    data: {
      issue: { id: ISSUE_ID, name: "Ship it", identifier: "ENG", sequence_id: 12 },
      issue_activity: {
        id: "623e4567-e89b-12d3-a456-426614174005",
        verb: "created",
        field: "comment",
        actor: ACTOR_ID,
        new_value: "",
        old_value: "",
        issue_comment: "looks good",
      },
    },
    entity_identifier: ISSUE_ID,
    entity_name: "issue",
    title: "",
    message: null,
    message_html: "",
    message_stripped: null,
    sender: "in_app:issue_activities:default",
    triggered_by: ACTOR_ID,
    triggered_by_details: {
      id: ACTOR_ID,
      first_name: "Ada",
      last_name: "Lovelace",
      avatar: "",
      avatar_url: null,
      display_name: "Ada Lovelace",
      is_bot: false,
    },
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

function page(rows: NotificationItem[]): NotificationListResponse {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: rows.length,
    next_cursor: "300:0:0",
    prev_cursor: "300:-1:0",
    next_page_results: false,
    prev_page_results: false,
    count: rows.length,
    total_results: rows.length,
    results: rows,
  };
}

function workspaceEntry(role: number) {
  return {
    id: WORKSPACE_ID,
    name: "Acme",
    slug: "acme",
    logo_url: null,
    total_members: 1,
    role,
  };
}

const UNREAD = { total_unread_notifications_count: 3, mention_unread_notifications_count: 1 };

interface ViewOptions {
  role?: number;
  seedWorkspace?: boolean;
  unread?: typeof UNREAD | null;
  defaultPage?: NotificationListResponse | null;
  mentionsPage?: NotificationListResponse | null;
  /**
   * When true the transport answers list/unread reads from the given
   * pages (nothing is seeded); otherwise the given data is seeded and
   * unseeded reads hang.
   */
  serveReads?: boolean;
}

function renderView(options: ViewOptions = {}): { calls: string[] } {
  const {
    role = ROLE_ADMIN,
    seedWorkspace = true,
    unread = UNREAD,
    defaultPage = null,
    mentionsPage = null,
    serveReads = false,
  } = options;
  const calls: string[] = [];
  const json = (payload: unknown, url: string) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(payload),
  });
  createAppClient("https://api.test", (async (url: string) => {
    calls.push(url);
    if (!serveReads) return new Promise<never>(() => {});
    if (url.includes("/users/notifications/unread/")) return json(unread ?? UNREAD, url);
    if (url.includes("/users/notifications")) {
      const body = url.includes("mentioned=true") ? (mentionsPage ?? page([])) : (defaultPage ?? page([]));
      return json(body, url);
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (seedWorkspace) {
    queryClient.setQueryData(sessionKeys.workspaces, [workspaceEntry(role)]);
  }
  if (!serveReads) {
    if (unread !== null) {
      queryClient.setQueryData(notificationKeys.unread("acme"), unread);
    }
    if (defaultPage !== null) {
      queryClient.setQueryData(notificationKeys.list("acme", notificationTabQuery("all")), defaultPage);
    }
    if (mentionsPage !== null) {
      queryClient.setQueryData(notificationKeys.list("acme", notificationTabQuery("mentions")), mentionsPage);
    }
  }
  render(
    <QueryClientProvider client={queryClient}>
      <InboxView workspaceSlug="acme" />
    </QueryClientProvider>
  );
  return { calls };
}

function cardTitles(): string[] {
  return screen
    .getAllByTestId("inbox-card")
    .map((card) => within(card).getByTestId("inbox-card-title").textContent ?? "");
}

beforeEach(() => {
  resetAppClient();
});

describe("InboxView", () => {
  it("renders the two-pane inbox with tabs, badges and newest-first cards", async () => {
    const older = row({
      id: "823e4567-e89b-12d3-a456-426614174007",
      created_at: "2026-10-16T11:00:00Z",
      read_at: "2026-10-16T11:30:00Z",
      data: {
        issue: { id: ISSUE_ID, name: "First story", identifier: "ENG", sequence_id: 11 },
        issue_activity: { verb: "created", field: "comment", issue_comment: "first" },
      },
    });
    const newer = row({
      id: "923e4567-e89b-12d3-a456-426614174008",
      created_at: "2026-10-16T12:00:00Z",
      data: {
        issue: { id: ISSUE_ID, name: "Second story", identifier: "ENG", sequence_id: 12 },
        issue_activity: { verb: "created", field: "comment", issue_comment: "second" },
      },
    });
    renderView({ defaultPage: page([newer, older]) });

    expect(screen.getByRole("region", { name: "Notifications" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Details" })).toBeInTheDocument();
    expect(screen.getByTestId("inbox-detail-host")).toBeInTheDocument();
    await waitFor(() => expect(document.title).toContain("Acme"));
    expect(document.title).toContain("Inbox");

    expect(screen.getByRole("tab", { name: /all/i })).toHaveAttribute("aria-selected", "true");
    expect(within(screen.getByTestId("inbox-tab-all")).getByText("3")).toBeInTheDocument();
    expect(within(screen.getByTestId("inbox-tab-mentions")).getByText("1")).toBeInTheDocument();

    expect(cardTitles()).toEqual(["Second story", "First story"]);
    const cards = screen.getAllByTestId("inbox-card");
    expect(cards[0]).toHaveAttribute("data-unread", "true");
    expect(cards[1]).toHaveAttribute("data-unread", "false");
  });

  it("switches streams on tab select, discarding the current list", async () => {
    const user = userEvent.setup();
    const plain = row({
      id: "a23e4567-e89b-12d3-a456-426614174009",
      data: {
        issue: { id: ISSUE_ID, name: "Plain story", identifier: "ENG", sequence_id: 10 },
        issue_activity: { verb: "created", field: "comment", issue_comment: "plain" },
      },
    });
    const mention = row({
      id: "b23e4567-e89b-12d3-a456-426614174010",
      is_mentioned_notification: true,
      data: {
        issue: { id: ISSUE_ID, name: "Mentioned story", identifier: "ENG", sequence_id: 13 },
        issue_activity: { verb: "created", field: "comment", issue_comment: "hey you" },
      },
    });
    const { calls } = renderView({
      unread: UNREAD,
      serveReads: true,
      defaultPage: page([plain]),
      mentionsPage: page([mention]),
    });
    expect(await screen.findByText("Plain story")).toBeInTheDocument();

    await user.click(screen.getByTestId("inbox-tab-mentions"));
    expect(await screen.findByText("Mentioned story")).toBeInTheDocument();
    expect(screen.queryByText("Plain story")).not.toBeInTheDocument();
    expect(screen.getByTestId("inbox-tab-mentions")).toHaveAttribute("aria-selected", "true");

    const lists = calls.filter((url) => url.includes("/users/notifications?"));
    expect(lists).toHaveLength(2);
    const params = lists.map((url) => new URL(url).searchParams);
    expect(params[0]?.get("mentioned")).toBeNull();
    expect(params[0]?.get("snoozed")).toBe("false");
    expect(params[0]?.get("archived")).toBe("false");
    expect(params[0]?.get("per_page")).toBe("300");
    expect(params[0]?.get("cursor")).toBe("300:0:0");
    expect(params[1]?.get("mentioned")).toBe("true");
  });

  it("talks only to the list and unread endpoints on entry (NTF-030)", async () => {
    const { calls } = renderView({ unread: null, serveReads: true });
    await screen.findByRole("tablist");
    await waitFor(() => expect(calls.length).toBe(2));
    const paths = calls.map((url) => new URL(url).pathname.replace(/\/+$/, ""));
    const base = "/api/workspaces/acme/users/notifications";
    expect(paths).toContain(base);
    expect(paths).toContain(`${base}/unread`);
    expect(paths.filter((path) => path !== base && path !== `${base}/unread`)).toEqual([]);
  });

  it("renders the same inbox for guests (NTF-026)", () => {
    renderView({ role: ROLE_GUEST, defaultPage: page([row()]), unread: UNREAD });
    expect(screen.getByRole("tab", { name: /all/i })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /mentions/i })).toBeInTheDocument();
    expect(cardTitles()).toEqual(["Ship it"]);
  });

  it("shows no cards while the list loads", () => {
    renderView({ unread: UNREAD });
    expect(screen.getByRole("region", { name: "Notifications" })).toBeInTheDocument();
    expect(screen.queryByTestId("inbox-card")).not.toBeInTheDocument();
    expect(within(screen.getByTestId("inbox-tab-all")).getByText("3")).toBeInTheDocument();
  });

  it("shows no cards when the stream is empty", () => {
    renderView({ defaultPage: page([]), unread: UNREAD });
    expect(screen.queryByTestId("inbox-card")).not.toBeInTheDocument();
  });

  it("hides the badges while the totals are unknown", () => {
    renderView({ unread: null, defaultPage: page([row()]) });
    expect(cardTitles()).toEqual(["Ship it"]);
    expect(within(screen.getByTestId("inbox-tab-all")).queryByText("3")).not.toBeInTheDocument();
    expect(within(screen.getByTestId("inbox-tab-mentions")).queryByText("1")).not.toBeInTheDocument();
  });

  it("skips rows that cannot be resolved", () => {
    const broken = row({ id: "c23e4567-e89b-12d3-a456-426614174011", project: null });
    renderView({ defaultPage: page([broken, row()]) });
    expect(cardTitles()).toEqual(["Ship it"]);
  });

  it("falls back to the slug title while the workspace loads", async () => {
    renderView({ seedWorkspace: false, defaultPage: page([row()]), unread: UNREAD });
    expect(cardTitles()).toEqual(["Ship it"]);
    await waitFor(() => expect(document.title).toContain("acme"));
    expect(document.title).toContain("Inbox");
  });
});
