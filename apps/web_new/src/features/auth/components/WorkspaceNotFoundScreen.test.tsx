// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const navigate = vi.fn();

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

function renderScreen(email: string, workspaceCount: number, signOutStatus = 200): { posts: string[] } {
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
      <WorkspaceNotFoundScreen email={email} workspaceCount={workspaceCount} />
    </QueryClientProvider>
  );
  return { posts };
}

afterEach(() => {
  resetAppClient();
  navigate.mockClear();
});

describe("WorkspaceNotFoundScreen", () => {
  it("names the account and offers home plus profile when the user belongs somewhere", () => {
    renderScreen("ada@example.com", 2);
    expect(screen.getByRole("heading", { name: "Workspace not found" })).toBeVisible();
    expect(screen.getByText("ada@example.com")).toBeVisible();
    const home = screen.getByRole("link", { name: "Go Home" });
    expect(home).toHaveAttribute("href", "/");
    expect(home).toHaveFocus();
    expect(screen.getByRole("link", { name: "Open profile" })).toHaveAttribute("href", "/settings/profile/general/");
  });

  it("offers creation when the user belongs nowhere", () => {
    renderScreen("ada@example.com", 0);
    const create = screen.getByRole("link", { name: "Create a workspace" });
    expect(create).toHaveAttribute("href", "/create-workspace");
    expect(create).toHaveFocus();
    expect(screen.queryByRole("link", { name: "Go Home" })).not.toBeInTheDocument();
  });

  it("signs out from the header and lands on the entry", async () => {
    const seen = renderScreen("ada@example.com", 1);
    await userEvent.setup().click(screen.getByRole("button", { name: "Sign out" }));
    await waitFor(() => expect(seen.posts).toHaveLength(1));
    await waitFor(() => expect(navigate).toHaveBeenCalledWith({ href: "/" }));
  });
});
