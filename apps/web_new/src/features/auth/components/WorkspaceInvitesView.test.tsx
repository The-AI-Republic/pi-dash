// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportInit, WorkspaceInvitationList } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError } from "../../../shared/shell/index.js";
import { INVITES_EMPTY_MESSAGE, INVITES_FAILED_TITLE, WorkspaceInvitesView } from "./WorkspaceInvitesView.js";

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

function invitation(id: string, name: string, role: number, logoUrl: string | null) {
  return {
    id,
    workspace: {
      id: "333e4567-e89b-12d3-a456-426614174002",
      name,
      slug: name.toLowerCase(),
      logo_url: logoUrl,
    },
    email: "ada@example.com",
    accepted: false,
    token: "token",
    message: null,
    responded_at: null,
    role,
    invite_link: "https://app.test/join",
    created_by: null,
    updated_by: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-01-02T03:04:05Z",
  };
}

const LIST_BODY = [
  {
    id: "333e4567-e89b-12d3-a456-426614174002",
    name: "Acme",
    slug: "acme",
    logo_url: null,
    total_members: 2,
    role: 15,
  },
];

const SETTINGS_BODY = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  email: "ada@example.com",
  workspace: {
    last_workspace_id: "333e4567-e89b-12d3-a456-426614174002",
    last_workspace_slug: "acme",
    fallback_workspace_id: null,
    fallback_workspace_slug: null,
    invites: 0,
  },
};

interface InvitesHarness {
  accepted: number;
  createdNew: number;
  posted: unknown;
  queryClient: QueryClient;
}

function renderView(options: { invitations?: WorkspaceInvitationList; failAccept?: boolean }): InvitesHarness {
  const harness: InvitesHarness = {
    accepted: 0,
    createdNew: 0,
    posted: undefined,
    queryClient: new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } }),
  };
  createAppClient(
    "https://api.test",
    stubTransport(async (url, init) => {
      if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
      if (url.endsWith("/api/users/me/workspaces/invitations/") && init.method === "POST") {
        harness.posted = JSON.parse(init.body as string);
        if (options.failAccept) return { status: 500, body: "{}", url };
        return { status: 204, body: "", url };
      }
      if (url.endsWith("/api/users/me/workspaces/")) {
        return { status: 200, body: JSON.stringify(LIST_BODY), url };
      }
      if (url.endsWith("/api/users/me/settings/")) {
        return { status: 200, body: JSON.stringify(SETTINGS_BODY), url };
      }
      return { status: 200, body: "{}", url };
    })
  );
  const invitations =
    options.invitations ??
    ([
      invitation("11111111-1111-4111-8111-111111111111", "Acme", 15, null),
      invitation("22222222-2222-4222-8222-222222222222", "Globex", 20, "https://cdn.test/globex.png"),
    ] as unknown as WorkspaceInvitationList);
  render(
    <QueryClientProvider client={harness.queryClient}>
      <WorkspaceInvitesView
        invitations={invitations}
        onAccepted={() => {
          harness.accepted += 1;
        }}
        onCreateNew={() => {
          harness.createdNew += 1;
        }}
      />
    </QueryClientProvider>
  );
  return harness;
}

afterEach(() => {
  resetAppClient();
  vi.mocked(notifyError).mockClear();
});

describe("WorkspaceInvitesView", () => {
  it("lists invite cards with submit disabled", () => {
    renderView({});
    expect(screen.getByRole("heading", { name: "Join a workspace" })).toBeInTheDocument();
    expect(screen.getByText("Acme")).toBeInTheDocument();
    expect(screen.getByText("Member")).toBeInTheDocument();
    expect(screen.getByText("Globex")).toBeInTheDocument();
    expect(screen.getByText("Admin")).toBeInTheDocument();
    expect(screen.getAllByRole("checkbox")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
  });

  it("toggles selection on card click", async () => {
    renderView({});
    const user = userEvent.setup();
    await user.click(screen.getByText("Acme"));
    expect(screen.getByRole("button", { name: "Continue" })).toBeEnabled();
    await user.click(screen.getByText("Acme"));
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
  });

  it("batch-accepts the selection, refreshes, and reports acceptance", async () => {
    const harness = renderView({});
    const user = userEvent.setup();
    fireEvent.click(screen.getAllByRole("checkbox")[0]!);
    fireEvent.click(screen.getAllByRole("checkbox")[1]!);
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(harness.accepted).toBe(1));
    expect(harness.posted).toEqual({
      invitations: ["11111111-1111-4111-8111-111111111111", "22222222-2222-4222-8222-222222222222"],
    });
    expect(harness.queryClient.getQueryData(["session", "workspaces"])).toEqual(LIST_BODY);
    expect(harness.queryClient.getQueryData(["session", "settings"])).toEqual(SETTINGS_BODY);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("toasts the server detail when accepting fails and stays on the view", async () => {
    const harness = renderView({ failAccept: true });
    const user = userEvent.setup();
    fireEvent.click(screen.getAllByRole("checkbox")[0]!);
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(notifyError).toHaveBeenCalledWith(INVITES_FAILED_TITLE, expect.any(String));
    expect(harness.accepted).toBe(0);
    expect(screen.getByRole("button", { name: "Continue" })).toBeEnabled();
  });

  it("returns to the create view through the ghost action", async () => {
    const harness = renderView({});
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Create a new workspace" }));
    expect(harness.createdNew).toBe(1);
    expect(harness.accepted).toBe(0);
  });

  it("renders the empty state with a way back to creating", async () => {
    const harness = renderView({ invitations: [] });
    expect(screen.getByText(INVITES_EMPTY_MESSAGE)).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Create a new workspace" }));
    expect(harness.createdNew).toBe(1);
  });
});
