// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError } from "../../../shared/shell/index.js";
import { JOIN_FAILED_TITLE, WorkspaceJoinByEmailView } from "./WorkspaceJoinByEmailView.js";

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

const LIST_BODY = [
  {
    id: "333e4567-e89b-12d3-a456-426614174002",
    name: "Acme",
    slug: "acme",
    logo_url: null,
    total_members: 1,
    role: 20,
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

interface JoinHarness {
  requested: string[];
  alreadyMember: number;
  backed: number;
  posted: unknown;
  queryClient: QueryClient;
}

function renderView(options: { answer?: unknown; fail?: boolean }): JoinHarness {
  const harness: JoinHarness = {
    requested: [],
    alreadyMember: 0,
    backed: 0,
    posted: undefined,
    queryClient: new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } }),
  };
  createAppClient(
    "https://api.test",
    stubTransport(async (url, init) => {
      if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
      if (url.endsWith("/api/users/me/workspaces/join-requests/") && init.method === "POST") {
        harness.posted = JSON.parse(init.body as string);
        if (options.fail) return { status: 500, body: "{}", url };
        return { status: 201, body: JSON.stringify(options.answer ?? { message: "Request sent" }), url };
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
  render(
    <QueryClientProvider client={harness.queryClient}>
      <WorkspaceJoinByEmailView
        onRequested={(email) => harness.requested.push(email)}
        onAlreadyMember={() => {
          harness.alreadyMember += 1;
        }}
        onBack={() => {
          harness.backed += 1;
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

describe("WorkspaceJoinByEmailView", () => {
  it("gates submit on a valid address", () => {
    renderView({});
    expect(screen.getByRole("heading", { name: "Request workspace access" })).toBeInTheDocument();
    const field = screen.getByLabelText("Admin email address");
    expect(document.activeElement).toBe(field);
    expect(screen.getByRole("button", { name: "Send join request" })).toBeDisabled();
    fireEvent.change(field, { target: { value: "not-an-email" } });
    expect(screen.getByRole("button", { name: "Send join request" })).toBeDisabled();
    fireEvent.change(field, { target: { value: "admin@example.com" } });
    expect(screen.getByRole("button", { name: "Send join request" })).toBeEnabled();
  });

  it("trims and posts the address, then reports the request", async () => {
    const harness = renderView({});
    const user = userEvent.setup();
    fireEvent.change(screen.getByLabelText("Admin email address"), { target: { value: "  admin@example.com " } });
    await user.click(screen.getByRole("button", { name: "Send join request" }));
    await waitFor(() => expect(harness.requested).toEqual(["admin@example.com"]));
    expect(harness.posted).toEqual({ admin_email: "admin@example.com" });
    expect(harness.alreadyMember).toBe(0);
  });

  it("submits with Enter", async () => {
    const harness = renderView({});
    const user = userEvent.setup();
    await user.type(screen.getByLabelText("Admin email address"), "admin@example.com{enter}");
    await waitFor(() => expect(harness.requested).toEqual(["admin@example.com"]));
  });

  it("takes the already-member shortcut when the answer names a workspace", async () => {
    const harness = renderView({ answer: { message: "Already a member", workspace_slug: "acme" } });
    const user = userEvent.setup();
    fireEvent.change(screen.getByLabelText("Admin email address"), { target: { value: "admin@example.com" } });
    await user.click(screen.getByRole("button", { name: "Send join request" }));
    await waitFor(() => expect(harness.alreadyMember).toBe(1));
    expect(harness.requested).toHaveLength(0);
    expect(harness.queryClient.getQueryData(["session", "workspaces"])).toEqual(LIST_BODY);
  });

  it("toasts the server detail when the request fails and stays on the view", async () => {
    const harness = renderView({ fail: true });
    const user = userEvent.setup();
    fireEvent.change(screen.getByLabelText("Admin email address"), { target: { value: "admin@example.com" } });
    await user.click(screen.getByRole("button", { name: "Send join request" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(notifyError).toHaveBeenCalledWith(JOIN_FAILED_TITLE, expect.any(String));
    expect(harness.requested).toHaveLength(0);
    expect(harness.alreadyMember).toBe(0);
    expect(screen.getByRole("button", { name: "Send join request" })).toBeEnabled();
  });

  it("returns to the create view through the ghost action", async () => {
    const harness = renderView({});
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Back to creating a workspace" }));
    expect(harness.backed).toBe(1);
  });
});
