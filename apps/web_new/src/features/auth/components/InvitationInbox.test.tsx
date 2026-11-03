// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportInit, WorkspaceInvitation } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { navigate } = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({ useNavigate: () => navigate }));

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError } from "../../../shared/shell/index.js";
import { authKeys } from "../queries.js";
import { BATCH_ACCEPT_FAILED_TITLE, BATCH_ACCEPT_STALE_MESSAGE, InvitationInbox } from "./InvitationInbox.js";

interface StubRoute {
  status: number;
  body: string;
  url: string;
}

function stubTransport(handler: (url: string, init: TransportInit) => StubRoute | Promise<StubRoute>): Transport {
  return async (url, init) => {
    const response = await handler(url, init);
    return {
      status: response.status,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
}

const CSRF = JSON.stringify({ csrf_token: "csrf" });

function inboxInvitation(overrides: Partial<WorkspaceInvitation> = {}): WorkspaceInvitation {
  return {
    id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    workspace: {
      id: "22222222-2222-4222-8222-222222222222",
      name: "Acme",
      slug: "acme",
      logo_url: null,
    },
    email: "ada@example.com",
    accepted: false,
    token: "token-acme",
    message: null,
    responded_at: null,
    role: 20,
    invite_link: "https://app.test/workspace-invitations?invitation_id=acme",
    created_by: null,
    updated_by: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

const ACME_INVITE = inboxInvitation();
const BETA_INVITE = inboxInvitation({
  id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
  workspace: {
    id: "33333333-3333-4333-8333-333333333333",
    name: "Beta",
    slug: "beta",
    logo_url: "https://cdn.test/beta.png",
  },
  role: 5,
  token: "token-beta",
});

function profileBody(): string {
  return JSON.stringify({
    id: "44444444-4444-4444-8444-444444444444",
    user: "55555555-5555-4555-8555-555555555555",
    theme: {},
    is_app_rail_docked: false,
    is_tour_completed: false,
    onboarding_step: {},
    use_case: null,
    role: null,
    is_onboarded: true,
    last_workspace_id: "22222222-2222-4222-8222-222222222222",
    billing_address_country: "",
    billing_address: null,
    has_billing_address: false,
    company_name: "",
    notification_view_mode: "",
    is_smooth_cursor_enabled: false,
    is_mobile_onboarded: false,
    mobile_onboarding_step: {},
    mobile_timezone_auto_set: false,
    language: "en",
    start_of_the_week: 0,
    goals: {},
    background_color: "",
    is_navigation_tour_completed: false,
    has_marketing_email_consent: false,
    is_subscribed_to_changelog: false,
    product_tour: {},
    settings: {},
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  });
}

const WORKSPACES_BODY = JSON.stringify([
  {
    id: "33333333-3333-4333-8333-333333333333",
    name: "Beta",
    slug: "beta",
    logo_url: null,
    total_members: 1,
    role: 20,
  },
]);

const SETTINGS_BODY = JSON.stringify({
  id: "123e4567-e89b-12d3-a456-426614174000",
  email: "ada@example.com",
  workspace: {
    last_workspace_id: "33333333-3333-4333-8333-333333333333",
    last_workspace_slug: "beta",
    fallback_workspace_id: null,
    fallback_workspace_slug: null,
    invites: 0,
  },
});

function renderInbox(transport: Transport): QueryClient {
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <InvitationInbox />
    </QueryClientProvider>
  );
  return queryClient;
}

function inboxTransport(invites: WorkspaceInvitation[]): Transport {
  return stubTransport(async (url, init) => {
    if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
    if (url.endsWith("/api/users/me/workspaces/invitations/") && init.method === "GET") {
      return { status: 200, body: JSON.stringify(invites), url };
    }
    if (url.endsWith("/api/users/me/workspaces/")) return { status: 200, body: WORKSPACES_BODY, url };
    if (url.endsWith("/api/users/me/settings/")) return { status: 200, body: SETTINGS_BODY, url };
    if (url.endsWith("/api/users/me/profile/")) return { status: 200, body: profileBody(), url };
    return { status: 204, body: "", url };
  });
}

afterEach(() => {
  resetAppClient();
  navigate.mockClear();
  vi.mocked(notifyError).mockClear();
});

describe("InvitationInbox", () => {
  it("loads the inbox list", async () => {
    let release!: (invites: WorkspaceInvitation[]) => void;
    const gate = new Promise<WorkspaceInvitation[]>((resolve) => {
      release = resolve;
    });
    renderInbox(
      stubTransport(async (url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        return { status: 200, body: JSON.stringify(await gate), url };
      })
    );

    expect(screen.getByRole("status")).not.toBeNull();
    expect(screen.getByText("Loading invitations")).not.toBeNull();
    release([ACME_INVITE]);
    await waitFor(() => expect(screen.getByRole("heading", { name: "Workspace invitations" })).not.toBeNull());
  });

  it("retries a failed load and offers a way home", async () => {
    let calls = 0;
    renderInbox(
      stubTransport(async (url) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        calls += 1;
        if (calls === 1) return { status: 500, body: JSON.stringify({ error: "boom" }), url };
        return { status: 200, body: JSON.stringify([ACME_INVITE]), url };
      })
    );

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Could not load invitations" })).not.toBeNull());
    await user.click(screen.getByRole("button", { name: "Back to home" }));
    expect(navigate).toHaveBeenCalledWith({ href: "/" });

    await user.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(screen.getByRole("heading", { name: "Workspace invitations" })).not.toBeNull());
  });

  it("shows the illustrated empty state with a way home", async () => {
    renderInbox(inboxTransport([]));

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("heading", { name: "No pending invites" })).not.toBeNull());
    expect(screen.getByRole("img", { name: /empty inbox tray/ })).toHaveAttribute(
      "src",
      "/illustrations/no-invites.svg"
    );
    await user.click(screen.getByRole("button", { name: "Back to home" }));
    expect(navigate).toHaveBeenCalledWith({ href: "/" });
  });

  it("lists one keyboard-reachable card per invite with workspace and role", async () => {
    renderInbox(inboxTransport([ACME_INVITE, BETA_INVITE]));

    await waitFor(() => expect(screen.getByRole("heading", { name: "Workspace invitations" })).not.toBeNull());
    const acme = screen.getByRole("button", { name: /Acme/ });
    const beta = screen.getByRole("button", { name: /Beta/ });
    expect(acme).toHaveAttribute("aria-pressed", "false");
    expect(beta).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByText("Admin")).not.toBeNull();
    expect(screen.getByText("Guest")).not.toBeNull();
    expect(screen.getByRole("button", { name: "Accept selected" })).toHaveAttribute("disabled");
  });

  it("toggles card selection", async () => {
    renderInbox(inboxTransport([ACME_INVITE]));

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Workspace invitations" })).not.toBeNull());
    const card = screen.getByRole("button", { name: /Acme/ });
    await user.click(card);
    expect(card).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Accept selected" })).not.toHaveAttribute("disabled");
    await user.click(card);
    expect(card).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("button", { name: "Accept selected" })).toHaveAttribute("disabled");
  });

  it("accepts every selected invite at once and lands in the first joined workspace", async () => {
    const posts: Array<{ url: string; body: unknown }> = [];
    let listCalls = 0;
    let releaseJoin!: () => void;
    const joinGate = new Promise<void>((resolve) => {
      releaseJoin = resolve;
    });
    renderInbox(
      stubTransport(async (url, init) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        if (url.endsWith("/api/users/me/workspaces/invitations/") && init.method === "GET") {
          listCalls += 1;
          return { status: 200, body: JSON.stringify([ACME_INVITE, BETA_INVITE]), url };
        }
        if (url.endsWith("/api/users/me/workspaces/")) return { status: 200, body: WORKSPACES_BODY, url };
        if (url.endsWith("/api/users/me/settings/")) return { status: 200, body: SETTINGS_BODY, url };
        if (url.endsWith("/api/users/me/workspaces/invitations/") && init.method === "POST") {
          posts.push({ url, body: init.body === undefined ? null : JSON.parse(init.body) });
          await joinGate;
          return { status: 204, body: "", url };
        }
        if (url.endsWith("/api/users/me/profile/")) {
          posts.push({ url, body: init.body === undefined ? null : JSON.parse(init.body) });
          return { status: 200, body: profileBody(), url };
        }
        return { status: 204, body: "", url };
      })
    );

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Workspace invitations" })).not.toBeNull());
    await user.click(screen.getByRole("button", { name: /Beta/ }));
    await user.click(screen.getByRole("button", { name: /Acme/ }));
    const accept = screen.getByRole("button", { name: "Accept selected" });
    await user.click(accept);

    await waitFor(() => expect(accept).toHaveAttribute("disabled"));
    expect(accept).toHaveAttribute("aria-busy", "true");
    releaseJoin();
    await waitFor(() => expect(navigate).toHaveBeenCalledWith({ href: "/beta" }));
    expect(posts).toEqual([
      {
        url: "https://api.test/api/users/me/workspaces/invitations/",
        body: { invitations: [BETA_INVITE.id, ACME_INVITE.id] },
      },
      {
        url: "https://api.test/api/users/me/profile/",
        body: { last_workspace_id: "33333333-3333-4333-8333-333333333333" },
      },
    ]);
    await waitFor(() => expect(listCalls).toBe(2));
  });

  it("stays on a failed stamp with an error toast", async () => {
    renderInbox(
      stubTransport(async (url, init) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        if (url.endsWith("/api/users/me/workspaces/invitations/") && init.method === "GET") {
          return { status: 200, body: JSON.stringify([ACME_INVITE]), url };
        }
        if (url.endsWith("/api/users/me/workspaces/")) return { status: 200, body: WORKSPACES_BODY, url };
        if (url.endsWith("/api/users/me/settings/")) return { status: 200, body: SETTINGS_BODY, url };
        if (url.endsWith("/api/users/me/profile/")) {
          return { status: 500, body: JSON.stringify({ error: "stamp exploded" }), url };
        }
        return { status: 204, body: "", url };
      })
    );

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Workspace invitations" })).not.toBeNull());
    await user.click(screen.getByRole("button", { name: /Acme/ }));
    await user.click(screen.getByRole("button", { name: "Accept selected" }));

    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(BATCH_ACCEPT_FAILED_TITLE, "stamp exploded"));
    expect(navigate).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Workspace invitations" })).not.toBeNull();
    expect(screen.getByRole("button", { name: "Accept selected" })).not.toHaveAttribute("disabled");
  });

  it("fails closed when the list no longer names the selection", async () => {
    let posted = false;
    const queryClient = renderInbox(
      stubTransport(async (url, init) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        if (url.endsWith("/api/users/me/workspaces/invitations/") && init.method === "GET") {
          return { status: 200, body: JSON.stringify([ACME_INVITE]), url };
        }
        if (init.method !== "GET") posted = true;
        return { status: 204, body: "", url };
      })
    );

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Workspace invitations" })).not.toBeNull());
    await user.click(screen.getByRole("button", { name: /Acme/ }));
    act(() => {
      queryClient.setQueryData(authKeys.myInvitations, [BETA_INVITE]);
    });
    await waitFor(() => expect(screen.getByRole("button", { name: /Beta/ })).not.toBeNull());
    await user.click(screen.getByRole("button", { name: "Accept selected" }));

    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(BATCH_ACCEPT_FAILED_TITLE, BATCH_ACCEPT_STALE_MESSAGE));
    expect(posted).toBe(false);
    expect(navigate).not.toHaveBeenCalled();
  });

  it("leaves for home without joining", async () => {
    renderInbox(inboxTransport([ACME_INVITE]));

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Workspace invitations" })).not.toBeNull());
    await user.click(screen.getByRole("button", { name: "Go home" }));
    expect(navigate).toHaveBeenCalledWith({ href: "/" });
  });

  it("stays on a failed join with an error toast", async () => {
    renderInbox(
      stubTransport(async (url, init) => {
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        if (url.endsWith("/api/users/me/workspaces/invitations/") && init.method === "GET") {
          return { status: 200, body: JSON.stringify([ACME_INVITE]), url };
        }
        return { status: 500, body: JSON.stringify({ error: "join exploded" }), url };
      })
    );

    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Workspace invitations" })).not.toBeNull());
    await user.click(screen.getByRole("button", { name: /Acme/ }));
    await user.click(screen.getByRole("button", { name: "Accept selected" }));

    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(BATCH_ACCEPT_FAILED_TITLE, "join exploded"));
    expect(navigate).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Workspace invitations" })).not.toBeNull();
  });
});
