// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { AssistantLayout } from "./AssistantLayout.js";

const localChat = vi.hoisted(() => ({ value: false }));

vi.mock("../../../core/platform/local-chat.js", () => ({ hasLocalChat: () => localChat.value }));

function renderLayout(): void {
  createAppClient("https://api.test", (async (url: string) => ({
    status: 200,
    url,
    headers: { get: () => "application/json" },
    text: async () => JSON.stringify([]),
  })) as never);
  const rootRoute = createRootRoute();
  const layout = createRoute({
    getParentRoute: () => rootRoute,
    path: "/$ws",
    component: () => <AssistantLayout workspaceSlug="acme" />,
  });
  const child = createRoute({
    getParentRoute: () => layout,
    path: "/assistant",
    component: () => <p>routed</p>,
  });
  const thread = createRoute({
    getParentRoute: () => layout,
    path: "/assistant/$threadId",
    component: () => <p>thread</p>,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([layout.addChildren([child, thread])]),
    history: createMemoryHistory({ initialEntries: ["/acme/assistant"] }),
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
});

describe("AssistantLayout", () => {
  it("renders the header actions beside the routed content without local chat", async () => {
    localChat.value = false;
    renderLayout();
    expect(await screen.findByText("routed")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Chat History" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "New chat" })).toHaveAttribute("href", "/acme/assistant");
    // No persistent rail: the conversation takes the full width and the
    // history surfaces only through the dialog.
    expect(screen.queryByTestId("assistant-rail")).toBeNull();
    expect(screen.queryByTestId("assistant-history-dialog")).toBeNull();
  });

  it("opens the history dialog from the header button", async () => {
    localChat.value = false;
    renderLayout();
    expect(await screen.findByText("routed")).toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole("button", { name: "Chat History" }));
    expect(await screen.findByTestId("assistant-history-dialog")).toBeInTheDocument();
    expect(await screen.findByText("No conversations yet.")).toBeInTheDocument();
  });

  it("renders the routed content alone when local chat is present", async () => {
    localChat.value = true;
    renderLayout();
    expect(await screen.findByText("routed")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Chat History" })).toBeNull();
    expect(screen.queryByRole("link", { name: "New chat" })).toBeNull();
  });
});
