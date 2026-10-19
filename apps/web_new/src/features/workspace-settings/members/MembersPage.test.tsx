// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members roster page (parity/specs/workspace-settings.md §7–§9,
// WSET-008/011/014): count chip, combined search + role filter, active-first
// order, row identity, skeleton, no-match, load failure and column sorting.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { MembersPage } from "./MembersPage.js";
import { formatJoiningDate } from "./MemberRow.js";
import { MEMBERS_NO_MATCH_TEXT } from "./MembersPage.js";

const SLUG = "acme";
const ADMIN_ID = "223e4567-e89b-12d3-a456-426614174001";
const MENTION_ID = "223e4567-e89b-12d3-a456-426614174002";
const GUEST_ID = "223e4567-e89b-12d3-a456-426614174003";
const GONE_ID = "223e4567-e89b-12d3-a456-426614174004";

function rosterRow(options: {
  id: string;
  userId: string;
  first: string;
  last: string;
  display: string;
  email: string;
  role: number;
  active?: boolean;
  bot?: boolean;
  medium?: string;
  avatarUrl?: string | null;
  created?: string;
}) {
  return {
    id: options.id,
    member: {
      id: options.userId,
      first_name: options.first,
      last_name: options.last,
      avatar: "",
      avatar_url: options.avatarUrl ?? null,
      display_name: options.display,
      is_bot: options.bot ?? false,
      email: options.email,
      last_login_medium: options.medium ?? "email",
    },
    role: options.role,
    is_active: options.active ?? true,
    created_at: options.created ?? "2026-02-01T00:00:00Z",
  };
}

const ROWS = [
  rosterRow({
    id: "323e4567-e89b-12d3-a456-426614174001",
    userId: ADMIN_ID,
    first: "Orion",
    last: "Admin",
    display: "Orion Admin",
    email: "oracle@example.com",
    role: 20,
    created: "2025-11-02T12:00:00Z",
  }),
  rosterRow({
    id: "323e4567-e89b-12d3-a456-426614174002",
    userId: MENTION_ID,
    first: "Parity",
    last: "Mention",
    display: "Parity Mention",
    email: "parity-mention@example.com",
    role: 15,
    created: "2026-01-15T12:00:00Z",
  }),
  rosterRow({
    id: "323e4567-e89b-12d3-a456-426614174003",
    userId: GUEST_ID,
    first: "Parity",
    last: "Guest",
    display: "Parity Guest",
    email: "parity-guest@example.com",
    role: 5,
    medium: "",
    avatarUrl: "/static/avatar.png",
    created: "2026-03-04T12:00:00Z",
  }),
  rosterRow({
    id: "323e4567-e89b-12d3-a456-426614174004",
    userId: GONE_ID,
    first: "Departed",
    last: "Person",
    display: "Departed Person",
    email: "departed@example.com",
    role: 15,
    active: false,
    created: "2025-08-01T12:00:00Z",
  }),
  rosterRow({
    id: "323e4567-e89b-12d3-a456-426614174005",
    userId: "223e4567-e89b-12d3-a456-426614174005",
    first: "Pi",
    last: "Bot",
    display: "Pi Bot",
    email: "bot@example.com",
    role: 15,
    bot: true,
  }),
];

interface StubOptions {
  roster?: unknown[];
  rosterStatus?: number;
  invitationsStatus?: number;
  hangRoster?: boolean;
  hangInvitations?: boolean;
  rosterDelayMs?: number;
}

function stubTransport(options: StubOptions = {}) {
  const calls: string[] = [];
  let rosterReads = 0;
  const transport = async (url: string) => {
    calls.push(url);
    const json = (status: number, body: unknown) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    });
    if (url.endsWith("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/${SLUG}/members/`)) {
      rosterReads += 1;
      if (options.hangRoster) return new Promise<never>(() => undefined);
      if (options.rosterDelayMs !== undefined && rosterReads === 1) {
        await new Promise((resolve) => setTimeout(resolve, options.rosterDelayMs));
      }
      const status = options.rosterStatus ?? 200;
      return json(status, status === 200 ? (options.roster ?? ROWS) : { error: "boom" });
    }
    if (url.endsWith(`/api/workspaces/${SLUG}/invitations/`)) {
      if (options.hangInvitations) return new Promise<never>(() => undefined);
      const status = options.invitationsStatus ?? 200;
      return json(status, status === 200 ? [] : { error: "boom" });
    }
    throw new Error(`unexpected request ${url}`);
  };
  return { calls, transport: transport as never };
}

function setup(options: StubOptions = {}) {
  const stub = stubTransport(options);
  createAppClient("https://api.test", stub.transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <MembersPage workspaceSlug={SLUG} />
    </QueryClientProvider>
  );
  return { ...stub, queryClient, ...view };
}

function emails(): string[] {
  return screen.queryAllByTestId("member-row").map((row) => row.getAttribute("data-email") ?? "");
}

function rowFor(email: string): HTMLElement {
  const row = screen.queryAllByTestId("member-row").find((candidate) => candidate.getAttribute("data-email") === email);
  if (row === undefined) throw new Error(`no row for ${email}`);
  return row;
}

afterEach(() => {
  resetAppClient();
});

describe("MembersPage loading states", () => {
  it("shows the skeleton until both reads resolve", async () => {
    setup({ rosterDelayMs: 40 });
    expect(screen.getByTestId("members-skeleton")).toBeInTheDocument();
    expect(screen.queryAllByTestId("member-row")).toHaveLength(0);
    await waitFor(() => expect(screen.queryByTestId("members-skeleton")).not.toBeInTheDocument());
    expect(emails().length).toBe(4);
  });

  it("holds the skeleton while invitations are still pending", async () => {
    setup({ hangInvitations: true });
    await waitFor(() => expect(screen.getByTestId("members-skeleton")).toBeInTheDocument());
    expect(screen.queryAllByTestId("member-row")).toHaveLength(0);
    expect(screen.queryByTestId("members-no-match")).not.toBeInTheDocument();
  });

  it("offers a retry when a read fails and recovers on the second try", async () => {
    let fail = true;
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    createAppClient("https://api.test", (async (url: string) => {
      const status = url.endsWith(`/api/workspaces/${SLUG}/members/`) && fail ? 500 : 200;
      if (url.endsWith("/auth/get-csrf-token/")) {
        return { status: 200, url, headers: { get: () => null }, text: async () => "{}" };
      }
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () =>
          JSON.stringify(url.endsWith(`/api/workspaces/${SLUG}/members/`) ? (status === 200 ? ROWS : {}) : []),
      };
    }) as never);
    render(
      <QueryClientProvider client={queryClient}>
        <MembersPage workspaceSlug={SLUG} />
      </QueryClientProvider>
    );
    expect(await screen.findByTestId("members-load-failed")).toHaveTextContent("Couldn't load the members.");
    fail = false;
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(screen.queryByTestId("members-load-failed")).not.toBeInTheDocument());
    expect(emails()).toHaveLength(4);
  });
});

describe("MembersPage roster", () => {
  it("shows the count chip without bots and every non-bot row active-first", async () => {
    setup();
    await waitFor(() => expect(screen.getByTestId("members-count")).toBeInTheDocument());
    expect(screen.getByTestId("members-count")).toHaveTextContent("4");
    expect(screen.queryByText("Pi Bot")).not.toBeInTheDocument();
    expect(emails()).toEqual([
      "oracle@example.com",
      "parity-mention@example.com",
      "parity-guest@example.com",
      "departed@example.com",
    ]);
  });

  it("combines the search with the role filter", async () => {
    setup();
    await waitFor(() => expect(emails()).toHaveLength(4));
    const search = screen.getByPlaceholderText("Search...");
    await userEvent.type(search, "parity");
    expect(emails()).toEqual(["parity-mention@example.com", "parity-guest@example.com"]);
    await userEvent.click(screen.getByRole("button", { name: "Filters" }));
    await userEvent.click(await screen.findByRole("button", { name: "Guest" }));
    expect(emails()).toEqual(["parity-guest@example.com"]);
    expect(screen.getByTestId("members-filter-active")).toBeInTheDocument();
    // The menu stays open across picks; clear the filter from inside it.
    await userEvent.click(screen.getByRole("button", { name: "Guest" }));
    expect(screen.queryByTestId("members-filter-active")).not.toBeInTheDocument();
    expect(emails()).toEqual(["parity-mention@example.com", "parity-guest@example.com"]);
  });

  it("matches suspended rows only under the Suspended filter", async () => {
    setup();
    await waitFor(() => expect(emails()).toHaveLength(4));
    await userEvent.click(screen.getByRole("button", { name: "Filters" }));
    expect((await screen.findAllByRole("button", { name: "Suspended" })).length).toBeGreaterThan(0);
    await userEvent.click(screen.getByRole("button", { name: "Suspended" }));
    expect(emails()).toEqual(["departed@example.com"]);
    expect(within(rowFor("departed@example.com")).getByText("Suspended")).toBeInTheDocument();
  });

  it("searches by email without touching the roster data", async () => {
    const { calls } = setup();
    await waitFor(() => expect(emails()).toHaveLength(4));
    const readsBefore = calls.filter((url) => url.endsWith(`/api/workspaces/${SLUG}/members/`)).length;
    fireEvent.change(screen.getByPlaceholderText("Search..."), { target: { value: "MENTION@" } });
    expect(emails()).toEqual(["parity-mention@example.com"]);
    fireEvent.change(screen.getByPlaceholderText("Search..."), { target: { value: "" } });
    expect(emails()).toHaveLength(4);
    const readsAfter = calls.filter((url) => url.endsWith(`/api/workspaces/${SLUG}/members/`)).length;
    expect(readsAfter).toBe(readsBefore);
  });

  it("shows the inline no-match note and restores the roster when the search clears", async () => {
    setup();
    await waitFor(() => expect(emails()).toHaveLength(4));
    await userEvent.type(screen.getByPlaceholderText("Search..."), "zzz-no-such-member-zzz");
    expect(screen.getByTestId("members-no-match")).toHaveTextContent(MEMBERS_NO_MATCH_TEXT);
    expect(emails()).toEqual([]);
    await userEvent.clear(screen.getByPlaceholderText("Search..."));
    expect(screen.queryByTestId("members-no-match")).not.toBeInTheDocument();
    expect(emails()).toHaveLength(4);
  });
});

describe("MembersPage row identity", () => {
  it("renders avatar, names, email, role, auth and joining date with a profile link", async () => {
    setup();
    await waitFor(() => expect(emails()).toHaveLength(4));
    const row = rowFor("parity-mention@example.com");
    const cells = within(row).getAllByRole("cell");
    expect(cells[0]).toHaveTextContent("Parity Mention");
    expect(cells[1]).toHaveTextContent("Parity Mention");
    expect(cells[2]).toHaveTextContent("parity-mention@example.com");
    expect(cells[3]).toHaveTextContent("Member");
    expect(cells[4]).toHaveTextContent("Email");
    expect(cells[5]).toHaveTextContent(formatJoiningDate("2026-01-15T12:00:00Z"));
    expect(within(row).getByRole("link", { name: "Open Parity Mention's profile" })).toHaveAttribute(
      "href",
      `/${SLUG}/profile/${MENTION_ID}/`
    );
    expect(row.querySelector('[data-avatar-kind="initial"]')).not.toBeNull();
  });

  it("uses the photo when the member set one and hides an unknown auth medium", async () => {
    setup();
    await waitFor(() => expect(emails()).toHaveLength(4));
    const guest = rowFor("parity-guest@example.com");
    expect(guest.querySelector('[data-avatar-kind="image"]')).not.toBeNull();
    expect(within(guest).getAllByRole("cell")[4]).toBeEmptyDOMElement();
    const link = within(guest).getByRole("link");
    expect(link).toHaveAttribute("href", `/${SLUG}/profile/${GUEST_ID}/`);
  });

  it("dims suspended rows: no link, no auth, no joining date, Suspended role", async () => {
    setup();
    await waitFor(() => expect(emails()).toHaveLength(4));
    const row = rowFor("departed@example.com");
    expect(within(row).queryByRole("link")).not.toBeInTheDocument();
    expect(row.querySelector('[data-avatar-kind="suspended"]')).not.toBeNull();
    const cells = within(row).getAllByRole("cell");
    expect(cells[3]).toHaveTextContent("Suspended");
    expect(cells[4]).toBeEmptyDOMElement();
    expect(cells[5]).toBeEmptyDOMElement();
    expect(row).toHaveAttribute("data-active", "false");
  });

  it("shows Admin and Guest roles for the other memberships", async () => {
    setup();
    await waitFor(() => expect(emails()).toHaveLength(4));
    expect(within(rowFor("oracle@example.com")).getAllByRole("cell")[3]).toHaveTextContent("Admin");
    expect(within(rowFor("parity-guest@example.com")).getAllByRole("cell")[3]).toHaveTextContent("Guest");
  });
});

describe("MembersPage column order", () => {
  it("orders inside the active-first rule and clears back to server order", async () => {
    setup();
    await waitFor(() => expect(emails()).toHaveLength(4));
    await userEvent.click(screen.getByRole("button", { name: "Sort by Display name" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "Z → A" }));
    expect(emails()).toEqual([
      "parity-mention@example.com",
      "parity-guest@example.com",
      "oracle@example.com",
      "departed@example.com",
    ]);
    await userEvent.click(screen.getByRole("button", { name: "Sort by Display name" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "Clear sorting" }));
    expect(emails()).toEqual([
      "oracle@example.com",
      "parity-mention@example.com",
      "parity-guest@example.com",
      "departed@example.com",
    ]);
  });

  it("orders by role and by joining date", async () => {
    setup();
    await waitFor(() => expect(emails()).toHaveLength(4));
    await userEvent.click(screen.getByRole("button", { name: "Sort by Account type" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "Guest → Admin" }));
    expect(emails()).toEqual([
      "parity-guest@example.com",
      "parity-mention@example.com",
      "oracle@example.com",
      "departed@example.com",
    ]);
    await userEvent.click(screen.getByRole("button", { name: "Sort by Joining date" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "Newest → Oldest" }));
    expect(emails()).toEqual([
      "parity-guest@example.com",
      "parity-mention@example.com",
      "oracle@example.com",
      "departed@example.com",
    ]);
  });

  it("has no sort menu on the authentication header", async () => {
    setup();
    await waitFor(() => expect(emails()).toHaveLength(4));
    expect(screen.getByRole("columnheader", { name: "Authentication" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Sort by Authentication" })).not.toBeInTheDocument();
  });
});

describe("MembersPage empty roster", () => {
  it("hides the count chip and shows no rows when only bots exist", async () => {
    setup({ roster: [ROWS[4]] });
    await waitFor(() => expect(screen.queryByTestId("members-skeleton")).not.toBeInTheDocument());
    expect(screen.queryByTestId("members-count")).not.toBeInTheDocument();
    expect(emails()).toEqual([]);
    expect(screen.getByTestId("members-no-match")).toBeInTheDocument();
  });

  it("keeps the count chip when every member is filtered out", async () => {
    setup();
    await waitFor(() => expect(emails()).toHaveLength(4));
    fireEvent.change(screen.getByPlaceholderText("Search..."), { target: { value: "nothing-here" } });
    expect(screen.getByTestId("members-count")).toHaveTextContent("4");
    expect(screen.getByTestId("members-no-match")).toBeInTheDocument();
  });

  it("orders active members above suspended ones when only two remain", async () => {
    setup({ roster: [ROWS[3], ROWS[1]] });
    await waitFor(() => expect(emails()).toHaveLength(2));
    expect(emails()).toEqual(["parity-mention@example.com", "departed@example.com"]);
  });

  it("keeps a row when the avatar path resolves relative to the API", async () => {
    setup();
    await waitFor(() => expect(emails()).toHaveLength(4));
    const image = rowFor("parity-guest@example.com").querySelector("img");
    expect(image).toHaveAttribute("src", "https://api.test/static/avatar.png");
  });

  it("treats an invitations read failure as a failed load", async () => {
    setup({ invitationsStatus: 500 });
    expect(await screen.findByTestId("members-load-failed")).toBeInTheDocument();
    expect(screen.queryByTestId("members-no-match")).not.toBeInTheDocument();
  });
});
