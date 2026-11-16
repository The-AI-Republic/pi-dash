// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// NTF-029 (parity/specs/notifications.md §11): the inbox answers only to
// pointer controls. Plausible keys, dragging a card, and any export or
// import gesture change nothing; no card is draggable and no export or
// import control exists in the list or its menus. Proven from the user's
// side: mount the inbox, try the gestures, and read what happened.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { NotificationItem } from "@pidash/api-client";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ROLE_ADMIN } from "../../../core/session/index.js";
import { sessionKeys } from "../../../core/session/queries.js";
import { notificationKeys } from "../api/keys.js";
import { InboxView } from "./InboxView.js";

const PROBE_KEYS = ["j", "k", "r", "e", "a", "ArrowDown", "ArrowUp", "Enter", "?"];

function row(title: string, id: string): NotificationItem {
  return {
    id,
    workspace: "323e4567-e89b-12d3-a456-426614174002",
    project: "223e4567-e89b-12d3-a456-426614174001",
    data: {
      issue: { id: "123e4567-e89b-12d3-a456-426614174000", name: title, identifier: "ENG", sequence_id: 12 },
      issue_activity: { verb: "created", field: "comment", issue_comment: title },
    },
    entity_identifier: "123e4567-e89b-12d3-a456-426614174000",
    entity_name: "issue",
    title: "",
    message: null,
    message_html: "",
    message_stripped: null,
    sender: "in_app:issue_activities:default",
    triggered_by: "423e4567-e89b-12d3-a456-426614174003",
    triggered_by_details: {
      id: "423e4567-e89b-12d3-a456-426614174003",
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
  };
}

const ROWS = [
  row("Probe first", "523e4567-e89b-12d3-a456-426614174004"),
  row("Probe second", "623e4567-e89b-12d3-a456-426614174005"),
];

function renderInbox(): { writes: string[] } {
  const writes: string[] = [];
  const json = (url: string, payload: unknown) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(payload),
  });
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return json(url, { csrf_token: "csrf" });
    if (init.method !== undefined && init.method !== "GET") {
      writes.push(`${init.method} ${url}`);
      return json(url, ROWS[0]);
    }
    if (url.includes("/users/notifications/unread/")) {
      return json(url, { total_unread_notifications_count: 2, mention_unread_notifications_count: 0 });
    }
    return json(url, { results: ROWS });
  }) as never);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(sessionKeys.workspaces, [
    {
      id: "323e4567-e89b-12d3-a456-426614174002",
      name: "Acme",
      slug: "acme",
      logo_url: null,
      total_members: 1,
      role: ROLE_ADMIN,
    },
  ]);
  client.setQueryData(notificationKeys.unread("acme"), {
    total_unread_notifications_count: 2,
    mention_unread_notifications_count: 0,
  });
  const listPage = {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: 2,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: 2,
    total_results: 2,
    results: ROWS,
  };
  client.setQueryData(notificationKeys.list("acme", { snoozed: false, archived: false }), listPage);
  client.setQueryData(notificationKeys.pages("acme", { snoozed: false, archived: false }), {
    pages: [listPage],
    pageParams: ["300:0:0"],
  });
  render(
    <QueryClientProvider client={client}>
      <InboxView workspaceSlug="acme" />
    </QueryClientProvider>
  );
  return { writes };
}

function titles(): string[] {
  return screen
    .queryAllByTestId("inbox-card")
    .map((card) => within(card).getByTestId("inbox-card-title").textContent ?? "");
}

beforeEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("NTF-029: pointer controls only", () => {
  it("okay: the inbox renders the cards the probes will try to move", () => {
    renderInbox();
    expect(titles()).toEqual(["Probe first", "Probe second"]);
  });

  it("keys change nothing: order, tab, cards and writes stay put", async () => {
    const user = userEvent.setup();
    const { writes } = renderInbox();
    const tab = screen.getByTestId("inbox-tab-all");
    await user.click(tab);
    for (const key of PROBE_KEYS) {
      const target = key === "Enter" ? tab : document.body;
      target.focus();
      await user.keyboard(`{${key}}`);
    }
    expect(titles()).toEqual(["Probe first", "Probe second"]);
    expect(tab).toHaveAttribute("aria-selected", "true");
    expect(within(tab).getByText("2")).toBeInTheDocument();
    expect(writes).toEqual([]);
  });

  it("cards cannot be dragged onto each other", async () => {
    renderInbox();
    const [first, second] = screen.getAllByTestId("inbox-card");
    for (const card of [first, second]) {
      expect(card).not.toHaveAttribute("draggable", "true");
    }
    fireEvent.dragStart(first as HTMLElement);
    fireEvent.dragOver(second as HTMLElement);
    fireEvent.drop(second as HTMLElement);
    expect(titles()).toEqual(["Probe first", "Probe second"]);
  });

  it("offers no export, import or download control anywhere in the list", () => {
    renderInbox();
    const pane = screen.getByTestId("inbox-list-pane");
    for (const role of ["button", "link", "menuitem"] as const) {
      expect(within(pane).queryAllByRole(role, { name: /export|import|download/i })).toEqual([]);
    }
    expect(within(pane).queryAllByText(/export|import|download/i)).toEqual([]);
  });

  it("offers no export path from a card's snooze menu either", async () => {
    const user = userEvent.setup();
    renderInbox();
    const [card] = screen.getAllByTestId("inbox-card");
    await user.click(within(card as HTMLElement).getByTestId("snooze-trigger"));
    const panel = await screen.findByTestId("snooze-panel");
    expect(within(panel).queryByText(/export|import|download/i)).not.toBeInTheDocument();
    expect(
      within(panel)
        .getAllByRole("button")
        .map((button) => button.textContent)
    ).toEqual(["1 day", "3 days", "5 days", "1 week", "2 weeks", "Custom date and time"]);
  });

  it("every action goes through a visible pointer control", async () => {
    const user = userEvent.setup();
    const { writes } = renderInbox();
    const [card] = screen.getAllByTestId("inbox-card");
    const row = within(card as HTMLElement).getByTestId("inbox-card-actions");
    for (const control of within(row).getAllByRole("button")) {
      expect(control).toBeVisible();
    }
    await user.click(within(card as HTMLElement).getByTestId("inbox-card-read-action"));
    await waitFor(() =>
      expect(writes).toEqual([
        "POST https://api.test/api/workspaces/acme/users/notifications/523e4567-e89b-12d3-a456-426614174004/read/",
      ])
    );
  });
});
