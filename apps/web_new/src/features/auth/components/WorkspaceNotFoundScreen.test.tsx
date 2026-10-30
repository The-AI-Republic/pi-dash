// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const navigate = vi.fn();
const runtimeHarness = vi.hoisted(() => ({
  dispose: vi.fn(async (_input: unknown) => undefined),
}));

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  const React = await import("react");
  return {
    ...original,
    useNavigate: () => navigate,
    Link: (props: { to: string; children?: React.ReactNode; [key: string]: unknown }) => {
      const { to, children, ...rest } = props;
      return React.createElement("a", { href: to, ...rest }, children);
    },
  };
});

vi.mock("../../../core/platform/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../core/platform/index.js")>();
  return { ...original, platform: { ...original.platform, kind: "desktop", agentRuntime: {} } };
});

vi.mock("../../runners/runtime/service.js", () => ({
  disposeRuntimeForSignOut: (input: unknown) => runtimeHarness.dispose(input),
}));

import { RuntimeSignOutLifecycle } from "../../runners/runtime/SignOutLifecycle.js";
import { WorkspaceNotFoundScreen } from "./WorkspaceNotFoundScreen.js";

function stubTransport(handler: (url: string) => { status: number; body: string; url: string }): Transport {
  return async (url) => {
    const response = handler(url);
    return {
      status: response.status,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
}

function renderScreen(
  email: string,
  workspaceCount: number,
  signOutStatus = 200,
  workspaceSlug: string | null = workspaceCount > 0 ? "acme" : null
): { posts: string[] } {
  const posts: string[] = [];
  createAppClient(
    "https://api.test",
    stubTransport((url) => {
      if (url.endsWith("/auth/get-csrf-token/"))
        return { status: 200, url, body: JSON.stringify({ csrf_token: "csrf" }) };
      if (url.endsWith("/auth/sign-out/")) {
        posts.push(url);
        return { status: signOutStatus, url, body: "{}" };
      }
      return { status: 404, url, body: "{}" };
    })
  );
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <RuntimeSignOutLifecycle />
      <WorkspaceNotFoundScreen email={email} workspaceCount={workspaceCount} workspaceSlug={workspaceSlug} />
    </QueryClientProvider>
  );
  return { posts };
}

afterEach(() => {
  resetAppClient();
  navigate.mockClear();
  runtimeHarness.dispose.mockClear();
});

describe("WorkspaceNotFoundScreen", () => {
  it("names the account and offers home plus profile when the user belongs somewhere", () => {
    renderScreen("ada@example.com", 2);
    expect(screen.getByRole("heading", { name: "Workspace not found" })).toBeVisible();
    expect(screen.getByText("ada@example.com")).toBeVisible();
    const home = screen.getByRole("link", { name: "Go Home" });
    expect(home).toHaveAttribute("href", "/");
    expect(home).toHaveFocus();
    // Account settings live inside a workspace (main#670): the profile
    // link hosts itself in the first workspace the reader belongs to.
    expect(screen.getByRole("link", { name: "Open profile" })).toHaveAttribute(
      "href",
      "/acme/settings/account/general/"
    );
  });

  it("offers creation when the user belongs nowhere", () => {
    renderScreen("ada@example.com", 0);
    const create = screen.getByRole("link", { name: "Create a workspace" });
    expect(create).toHaveAttribute("href", "/create-workspace");
    expect(create).toHaveFocus();
    expect(screen.queryByRole("link", { name: "Go Home" })).not.toBeInTheDocument();
  });

  it("confirms and disposes the desktop runtime before signing out from the non-gated header", async () => {
    const seen = renderScreen("ada@example.com", 1);
    await userEvent.setup().click(screen.getByRole("button", { name: "Sign out" }));
    const dialog = await screen.findByRole("dialog", { name: "Sign out?" });
    expect(seen.posts).toHaveLength(0);
    await userEvent.setup().click(within(dialog).getByRole("button", { name: "Sign out" }));
    await waitFor(() =>
      expect(runtimeHarness.dispose).toHaveBeenCalledWith(
        expect.objectContaining({ choice: { deleteChatHistory: false }, runtime: {} })
      )
    );
    await waitFor(() => expect(seen.posts).toHaveLength(1));
    await waitFor(() => expect(navigate).toHaveBeenCalledWith({ href: "/login" }));
  });
});
