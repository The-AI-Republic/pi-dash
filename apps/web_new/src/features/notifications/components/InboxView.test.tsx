// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { NotificationItem, NotificationListResponse } from "@pidash/api-client";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifySuccess: vi.fn(), notifyError: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ROLE_ADMIN, ROLE_GUEST } from "../../../core/session/index.js";
import { sessionKeys } from "../../../core/session/queries.js";
import { FULL_SCREEN_PORTAL_ID, notifySuccess } from "../../../shared/shell/index.js";
import { notificationKeys } from "../api/keys.js";
import {
  MARKED_ARCHIVED_TITLE,
  MARKED_READ_TITLE,
  SNOOZED_TITLE,
  UNARCHIVED_TITLE,
  UNSNOOZED_TITLE,
} from "../api/mutations.js";
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
  nextPage?: NotificationListResponse | null;
  nextPageDelayMs?: number;
  failFirstRead?: boolean;
  firstReadFailureDelayMs?: number;
  /**
   * When true the transport answers list/unread reads from the given
   * pages (nothing is seeded); otherwise the given data is seeded and
   * unseeded reads hang.
   */
  serveReads?: boolean;
}

function renderView(options: ViewOptions = {}): {
  calls: string[];
  firstReadDidFail: () => boolean;
  unmount: () => void;
} {
  if (document.getElementById(FULL_SCREEN_PORTAL_ID) === null) {
    const portal = document.createElement("div");
    portal.id = FULL_SCREEN_PORTAL_ID;
    document.body.appendChild(portal);
  }
  const {
    role = ROLE_ADMIN,
    seedWorkspace = true,
    unread = UNREAD,
    defaultPage = null,
    mentionsPage = null,
    nextPage = null,
    nextPageDelayMs = 0,
    failFirstRead = false,
    firstReadFailureDelayMs = 0,
    serveReads = false,
  } = options;
  const calls: string[] = [];
  let readWrites = 0;
  let firstReadFailed = false;
  const json = (payload: unknown, url: string) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(payload),
  });
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    calls.push(url);
    if (!serveReads) return new Promise<never>(() => {});
    if (url.endsWith("/auth/get-csrf-token/")) return json({ csrf_token: "csrf" }, url);
    if (init.method === "POST" && /\/users\/notifications\/[^/]+\/read\/$/.test(new URL(url).pathname)) {
      readWrites += 1;
      if (failFirstRead && readWrites === 1) {
        if (firstReadFailureDelayMs > 0) {
          await new Promise((resolve) => setTimeout(resolve, firstReadFailureDelayMs));
        }
        firstReadFailed = true;
        return { ...json({ detail: "read failed" }, url), status: 500 };
      }
      const notificationId = new URL(url).pathname.split("/").at(-3) ?? row().id;
      return json(row({ id: notificationId, read_at: "2026-10-17T12:30:00Z" }), url);
    }
    if (url.includes("/project-roles/")) return json({ [PROJECT_ID]: ROLE_ADMIN }, url);
    if (url.includes(`/projects/${PROJECT_ID}/issues/${ISSUE_ID}/`)) {
      return { ...json({ detail: "not hydrated in this integration fixture" }, url), status: 404 };
    }
    if (url.includes("/users/notifications/unread/")) return json(unread ?? UNREAD, url);
    if (url.includes("/users/notifications")) {
      const cursor = new URL(url).searchParams.get("cursor");
      if (cursor !== "300:0:0" && nextPage !== null) {
        if (nextPageDelayMs > 0) await new Promise((resolve) => setTimeout(resolve, nextPageDelayMs));
        return json(nextPage, url);
      }
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
      queryClient.setQueryData(notificationKeys.pages("acme", notificationTabQuery("all")), {
        pages: [defaultPage],
        pageParams: ["300:0:0"],
      });
    }
    if (mentionsPage !== null) {
      queryClient.setQueryData(notificationKeys.list("acme", notificationTabQuery("mentions")), mentionsPage);
      queryClient.setQueryData(notificationKeys.pages("acme", notificationTabQuery("mentions")), {
        pages: [mentionsPage],
        pageParams: ["300:0:0"],
      });
    }
  }
  const rendered = render(
    <QueryClientProvider client={queryClient}>
      <InboxView workspaceSlug="acme" />
    </QueryClientProvider>
  );
  return { calls, firstReadDidFail: () => firstReadFailed, unmount: rendered.unmount };
}

function cardTitles(): string[] {
  return screen
    .queryAllByTestId("inbox-card")
    .map((card) => within(card).getByTestId("inbox-card-title").textContent ?? "");
}

beforeEach(() => {
  resetAppClient();
  vi.clearAllMocks();
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
    expect(screen.getByTestId("notifications-detail-placeholder")).toBeInTheDocument();
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

  it("shows skeleton rows while the first page loads", () => {
    renderView({ unread: UNREAD });
    expect(screen.getByRole("region", { name: "Notifications" })).toBeInTheDocument();
    expect(screen.queryByTestId("inbox-card")).not.toBeInTheDocument();
    expect(screen.getByTestId("notifications-skeleton")).toHaveAccessibleName("Loading notifications");
    expect(within(screen.getByTestId("inbox-tab-all")).getByText("3")).toBeInTheDocument();
  });

  it("shows the full-stream empty illustration when the stream is empty", () => {
    renderView({ defaultPage: page([]), unread: UNREAD });
    expect(screen.queryByTestId("inbox-card")).not.toBeInTheDocument();
    expect(screen.getByTestId("notifications-empty-state")).toHaveAttribute("data-empty-tab", "all");
    expect(screen.getByText("Nothing has arrived")).toBeInTheDocument();
    expect(screen.queryByTestId("notifications-skeleton")).not.toBeInTheDocument();
  });

  it("shows a distinct mentions empty state after switching tabs", async () => {
    const user = userEvent.setup();
    renderView({ serveReads: true, defaultPage: page([]), mentionsPage: page([]) });
    await screen.findByText("Nothing has arrived");
    await user.click(screen.getByTestId("inbox-tab-mentions"));
    expect(await screen.findByText("No mentions waiting")).toBeInTheDocument();
    expect(screen.getByTestId("notifications-empty-state")).toHaveAttribute("data-empty-tab", "mentions");
  });

  it("loads and appends an older page through the explicit busy control", async () => {
    const user = userEvent.setup();
    const first = page([
      row({
        data: {
          issue: { id: ISSUE_ID, name: "Newest story", identifier: "ENG", sequence_id: 12 },
          issue_activity: { verb: "created", field: "comment", issue_comment: "new" },
        },
      }),
    ]);
    first.next_page_results = true;
    first.next_cursor = "300:300:1";
    const older = page([
      row({
        id: "823e4567-e89b-12d3-a456-426614174007",
        data: {
          issue: { id: ISSUE_ID, name: "Older story", identifier: "ENG", sequence_id: 11 },
          issue_activity: { verb: "created", field: "comment", issue_comment: "old" },
        },
      }),
    ]);
    renderView({ serveReads: true, defaultPage: first, nextPage: older, nextPageDelayMs: 50 });
    expect(await screen.findByText("Newest story")).toBeInTheDocument();
    const control = screen.getByTestId("notifications-next-page");
    await user.click(control);
    expect(control).toHaveTextContent("Loading older notifications");
    expect(await screen.findByText("Older story")).toBeInTheDocument();
    expect(cardTitles()).toEqual(["Newest story", "Older story"]);
    expect(screen.queryByTestId("notifications-next-page")).not.toBeInTheDocument();
  });

  it("selects locally, marks an unread card once, closes, and remounts unselected", async () => {
    const user = userEvent.setup();
    const first = renderView({ serveReads: true, defaultPage: page([row()]) });
    const card = await screen.findByTestId("inbox-card");
    expect(screen.getByTestId("notifications-detail-placeholder")).toBeInTheDocument();
    await user.click(card);
    await waitFor(() => expect(card).toHaveAttribute("data-unread", "false"));
    await waitFor(() =>
      expect(screen.getByTestId("notifications-detail-pane")).toHaveAttribute("data-detail-variant", "peek")
    );
    expect(screen.getByTestId("inbox-list-pane")).toHaveClass("hidden", "md:flex");
    expect(first.calls.filter((url) => /\/notifications\/[^/]+\/read\/$/.test(new URL(url).pathname))).toHaveLength(1);

    await user.click(screen.getByRole("button", { name: "Close preview" }));
    expect(screen.getByTestId("notifications-detail-placeholder")).toBeInTheDocument();
    await user.click(card);
    await waitFor(() =>
      expect(first.calls.filter((url) => /\/notifications\/[^/]+\/read\/$/.test(new URL(url).pathname))).toHaveLength(1)
    );
    expect(window.location.search).toBe("");

    first.unmount();
    renderView({ serveReads: true, defaultPage: page([row({ read_at: "2026-10-17T12:30:00Z" })]) });
    expect(await screen.findByTestId("inbox-card")).toBeInTheDocument();
    expect(screen.getByTestId("notifications-detail-placeholder")).toBeInTheDocument();
  });

  it("allows a failed first-open read to retry", async () => {
    const user = userEvent.setup();
    const { calls } = renderView({ serveReads: true, defaultPage: page([row()]), failFirstRead: true });
    const card = await screen.findByTestId("inbox-card");
    await user.click(card);
    await waitFor(() =>
      expect(calls.filter((url) => /\/notifications\/[^/]+\/read\/$/.test(new URL(url).pathname))).toHaveLength(1)
    );
    await user.click(card);
    await waitFor(() => expect(card).toHaveAttribute("data-unread", "false"));
    expect(calls.filter((url) => /\/notifications\/[^/]+\/read\/$/.test(new URL(url).pathname))).toHaveLength(2);
  });

  it("allows an earlier failed read to retry after another card starts reading", async () => {
    const user = userEvent.setup();
    const second = row({ id: "823e4567-e89b-12d3-a456-426614174007" });
    const { calls, firstReadDidFail } = renderView({
      serveReads: true,
      defaultPage: page([row(), second]),
      failFirstRead: true,
      firstReadFailureDelayMs: 50,
    });
    const [firstCard, secondCard] = await screen.findAllByTestId("inbox-card");
    if (firstCard === undefined || secondCard === undefined) throw new Error("expected two notification cards");

    await user.click(firstCard);
    await user.click(secondCard);
    await waitFor(() => expect(firstReadDidFail()).toBe(true));
    await user.click(firstCard);

    await waitFor(() =>
      expect(calls.filter((url) => /\/notifications\/[^/]+\/read\/$/.test(new URL(url).pathname))).toHaveLength(3)
    );
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

interface WriteScript {
  /** Per-call bit: status for each per-item write, in call order. */
  statuses?: number[];
}

/** Stub transport that answers the two entry reads plus per-item writes. */
function renderWritable(options: {
  rows: NotificationItem[];
  unread?: { total_unread_notifications_count: number; mention_unread_notifications_count: number };
  script?: WriteScript;
}): { writes: Array<{ method: string; url: string; body: string }> } {
  const unread = options.unread ?? UNREAD;
  const server = new Map(options.rows.map((entry) => [entry.id, entry]));
  const writes: Array<{ method: string; url: string; body: string }> = [];
  const json = (url: string, payload: unknown) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(payload),
  });
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return json(url, { csrf_token: "csrf" });
    const method = init.method ?? "GET";
    if (url.includes("/users/notifications/unread/")) return json(url, unread);
    if (url.includes("/users/notifications") && method === "GET") {
      const body = url.includes("mentioned=true") ? page([]) : page([...server.values()]);
      return json(url, body);
    }
    const item = [...server.values()].find((entry) => url.includes(entry.id));
    if (item === undefined) throw new Error(`unexpected write ${url}`);
    writes.push({ method, url, body: init.body ?? "{}" });
    const status = options.script?.statuses?.[writes.length - 1] ?? 200;
    if (status !== 200) {
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ detail: "parity write failure" }),
      };
    }
    let next: NotificationItem = item;
    if (url.endsWith("/read/")) next = { ...item, read_at: method === "POST" ? "2026-10-17T10:00:00Z" : null };
    else if (url.endsWith("/archive/")) {
      next = { ...item, archived_at: method === "POST" ? "2026-10-17T10:00:00Z" : null };
    } else {
      next = { ...item, snoozed_till: (JSON.parse(init.body ?? "{}") as { snoozed_till: string | null }).snoozed_till };
    }
    server.set(item.id, next);
    return json(url, next);
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  queryClient.setQueryData(sessionKeys.workspaces, [workspaceEntry(ROLE_ADMIN)]);
  queryClient.setQueryData(notificationKeys.unread("acme"), unread);
  render(
    <QueryClientProvider client={queryClient}>
      <InboxView workspaceSlug="acme" />
    </QueryClientProvider>
  );
  return { writes };
}

function firstCard(): HTMLElement {
  const [card] = screen.getAllByTestId("inbox-card");
  if (card === undefined) throw new Error("no card rendered");
  return card;
}

function storyRow(title: string, overrides: Partial<NotificationItem> = {}): NotificationItem {
  return row({
    data: {
      issue: { id: ISSUE_ID, name: title, identifier: "ENG", sequence_id: 12 },
      issue_activity: { verb: "created", field: "comment", issue_comment: title },
    },
    ...overrides,
  });
}

describe("InboxView card actions", () => {
  it("marks a card read from its hover row and toasts", async () => {
    const user = userEvent.setup();
    const { writes } = renderWritable({ rows: [storyRow("Ship it")] });
    await screen.findByText("Ship it");

    await user.click(within(firstCard()).getByTestId("inbox-card-read-action"));

    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(MARKED_READ_TITLE));
    expect(writes).toHaveLength(1);
    expect(writes[0]?.method).toBe("POST");
    expect(writes[0]?.url).toContain("/read/");
    await waitFor(() => expect(screen.getByTestId("inbox-card")).toHaveAttribute("data-unread", "false"));
  });

  it("leaves a card in place when its read write fails", async () => {
    const user = userEvent.setup();
    renderWritable({ rows: [storyRow("Ship it")], script: { statuses: [500] } });
    await screen.findByText("Ship it");

    await user.click(within(firstCard()).getByTestId("inbox-card-read-action"));

    await waitFor(() => expect(within(firstCard()).getByTestId("inbox-card-read-action")).not.toBeDisabled());
    expect(firstCard()).toHaveAttribute("data-unread", "true");
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("archives a card out of the default stream", async () => {
    const user = userEvent.setup();
    renderWritable({
      rows: [storyRow("Ship it"), storyRow("Keep me", { id: "a23e4567-e89b-12d3-a456-426614174009" })],
    });
    await screen.findByText("Ship it");

    const [ship] = screen.getAllByTestId("inbox-card");
    await user.click(within(ship as HTMLElement).getByTestId("inbox-card-archive-action"));

    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(MARKED_ARCHIVED_TITLE));
    await waitFor(() => expect(cardTitles()).toEqual(["Keep me"]));
  });

  it("snoozes a card from the menu with the preset's delay", async () => {
    const user = userEvent.setup();
    const { writes } = renderWritable({ rows: [storyRow("Ship it")] });
    await screen.findByText("Ship it");

    await user.click(within(firstCard()).getByTestId("snooze-trigger"));
    await user.click(await screen.findByTestId("snooze-option-day-1"));

    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(SNOOZED_TITLE));
    expect(writes[0]?.method).toBe("PATCH");
    const sent = JSON.parse(writes[0]?.body ?? "{}") as { snoozed_till: string };
    const delta = new Date(sent.snoozed_till).getTime() - Date.now();
    expect(delta).toBeGreaterThan(24 * 60 * 60 * 1000 - 60_000);
    expect(delta).toBeLessThan(24 * 60 * 60 * 1000 + 60_000);
    await waitFor(() => expect(cardTitles()).toEqual([]));
  });

  it("keeps a card in place when its snooze write fails", async () => {
    const user = userEvent.setup();
    renderWritable({ rows: [storyRow("Ship it")], script: { statuses: [500] } });
    await screen.findByText("Ship it");

    await user.click(within(firstCard()).getByTestId("snooze-trigger"));
    await user.click(await screen.findByTestId("snooze-option-day-3"));

    await waitFor(() => expect(within(firstCard()).getByTestId("inbox-card-read-action")).not.toBeDisabled());
    expect(cardTitles()).toEqual(["Ship it"]);
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("snoozes to the moment picked in the custom dialog", async () => {
    const user = userEvent.setup();
    const { writes } = renderWritable({ rows: [storyRow("Ship it")] });
    await screen.findByText("Ship it");

    await user.click(within(firstCard()).getByTestId("snooze-trigger"));
    await user.click(await screen.findByTestId("snooze-option-custom"));
    await screen.findByTestId("snooze-custom-dialog");
    await user.click(screen.getByRole("button", { name: "Pick a date" }));
    const target = new Date();
    target.setDate(target.getDate() + 2);
    await user.click(
      await screen.findByRole("button", {
        name: new RegExp(`^\\w+ ${target.getDate()}, ${target.getFullYear()}$`),
      })
    );
    await user.click(screen.getByRole("button", { name: "Pick a time" }));
    const panel = await screen.findByRole("listbox", { name: "Pick a time" });
    await user.click(within(panel).getByRole("button", { name: /^AM$/ }));
    await user.click(within(panel).getByRole("option", { name: "10:30" }));
    await user.click(screen.getByRole("button", { name: "Submit" }));

    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(SNOOZED_TITLE));
    expect(writes[0]?.method).toBe("PATCH");
    const sent = JSON.parse(writes[0]?.body ?? "{}") as { snoozed_till: string };
    const expected = new Date(target.getFullYear(), target.getMonth(), target.getDate(), 10, 30);
    expect(sent.snoozed_till).toBe(expected.toISOString());
    await waitFor(() => expect(screen.queryByTestId("snooze-custom-dialog")).not.toBeInTheDocument());
    await waitFor(() => expect(cardTitles()).toEqual([]));
  });

  it("unarchives a card from archived mode and toasts", async () => {
    const user = userEvent.setup();
    const archived = storyRow("Put away", {
      id: "d23e4567-e89b-12d3-a456-426614174012",
      archived_at: "2026-10-17T09:00:00Z",
    });
    renderWritable({ rows: [storyRow("Ship it"), archived] });
    await screen.findByText("Ship it");

    const putAway = screen.getByRole("article", { name: "Put away (unread)" });
    await user.click(within(putAway).getByTestId("inbox-card-archive-action"));

    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(UNARCHIVED_TITLE));
    const [ship, returned] = screen.getAllByTestId("inbox-card");
    expect(within(ship as HTMLElement).getByTestId("inbox-card-title")).toHaveTextContent("Ship it");
    expect(within(returned as HTMLElement).queryByTestId("inbox-card-archive-action")).toHaveAttribute(
      "aria-label",
      "Archive"
    );
  });

  it("removes an existing snooze from the menu", async () => {
    const user = userEvent.setup();
    const snoozed = storyRow("Nap time", {
      id: "e23e4567-e89b-12d3-a456-426614174013",
      snoozed_till: "2026-10-19T17:30:00Z",
    });
    const { writes } = renderWritable({ rows: [snoozed] });
    await screen.findByText("Nap time");

    await user.click(within(firstCard()).getByTestId("snooze-trigger"));
    await user.click(await screen.findByTestId("snooze-option-unsnooze"));

    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(UNSNOOZED_TITLE));
    expect(JSON.parse(writes[0]?.body ?? "{}")).toEqual({ snoozed_till: null });
  });
});
