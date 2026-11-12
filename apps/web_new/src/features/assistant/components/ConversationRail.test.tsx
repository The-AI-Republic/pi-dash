// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation rail (AGT-044 rail half + AGT-041 guest half): newest-first
// links with an untitled fallback, the open row highlighted, archived rows
// hidden, the placeholder only when the read resolved empty, and nothing
// at all when the read was refused.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { render, screen, waitFor } from "@testing-library/react";
import type { AssistantThread } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { assistantKeys } from "../api/keys.js";
import { ConversationRail } from "./ConversationRail.js";

function makeThread(id: string, overrides: Partial<AssistantThread> = {}): AssistantThread {
  return {
    id,
    title: `Thread ${id}`,
    is_archived: false,
    has_active_turn: false,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-02T00:00:00Z",
    ...overrides,
  };
}

function renderRail(options: { threads?: AssistantThread[]; fail?: boolean; initialPath?: string }): void {
  createAppClient("https://api.test", (async (url: string) => ({
    status: options.fail === true ? 403 : 200,
    url,
    headers: { get: () => "application/json" },
    text: async () => JSON.stringify(options.fail === true ? { error: "role_not_allowed" } : (options.threads ?? [])),
  })) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (options.threads !== undefined) queryClient.setQueryData(assistantKeys.threads("acme"), options.threads);
  const rootRoute = createRootRouteWithContext<{ queryClient: QueryClient }>()();
  const wsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/$ws",
    component: () => <Outlet />,
  });
  const layoutRoute = createRoute({
    getParentRoute: () => wsRoute,
    id: "assistant",
    component: () => (
      <div>
        <ConversationRail workspaceSlug="acme" />
        <Outlet />
      </div>
    ),
  });
  const landing = createRoute({
    getParentRoute: () => layoutRoute,
    path: "/assistant",
    component: () => <div data-testid="landing-slot" />,
  });
  const conversation = createRoute({
    getParentRoute: () => layoutRoute,
    path: "/assistant/$threadId",
    component: () => <div data-testid="conversation-slot" />,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([wsRoute.addChildren([layoutRoute.addChildren([landing, conversation])])]),
    context: { queryClient },
    history: createMemoryHistory({ initialEntries: [options.initialPath ?? "/acme/assistant"] }),
  });
  // Serve the loader-less rail through the cache so the pending/refused
  // branches are observable.
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
});

describe("ConversationRail", () => {
  it("lists visible conversations newest-first and highlights the open one", async () => {
    renderRail({
      threads: [
        makeThread("b", { title: "Beta" }),
        makeThread("gone", { is_archived: true }),
        makeThread("a", { title: "" }),
      ],
      initialPath: "/acme/assistant/b",
    });
    expect(await screen.findByRole("link", { name: "Beta" })).toHaveAttribute("href", "/acme/assistant/b");
    const untitled = screen.getByRole("link", { name: "Untitled conversation" });
    expect(untitled).toHaveAttribute("href", "/acme/assistant/a");
    expect(screen.queryByRole("link", { name: "Thread gone" })).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Beta" })).toHaveAttribute("aria-current", "page");
    expect(untitled).not.toHaveAttribute("aria-current");
    expect(screen.getByRole("link", { name: "New chat" })).toHaveAttribute("href", "/acme/assistant");
    expect(screen.queryByText("No conversations yet.")).not.toBeInTheDocument();
  });

  it("brands its own link rail and keeps thread management out of it", async () => {
    renderRail({ threads: [makeThread("b", { title: "Beta" })] });
    expect(await screen.findByTestId("assistant-rail-header")).toHaveTextContent("Pi Dash AI");
    // The rail is links only: no buttons, and no rename/archive/delete
    // affordance anywhere in the chat surface (AGT-058/AGT-061).
    const rail = screen.getByTestId("assistant-rail");
    expect(rail.querySelectorAll("button")).toHaveLength(0);
    for (const link of Array.from(rail.querySelectorAll("a"))) {
      expect(link.textContent ?? "").not.toMatch(/rename|archive|delete/i);
    }
    expect(rail.querySelector("a")?.tagName).toBe("A");
  });

  it("shows the placeholder only for a resolved empty list", async () => {
    renderRail({ threads: [] });
    expect(await screen.findByText("No conversations yet.")).toBeInTheDocument();
  });

  it("shows neither rows nor the placeholder when the read is refused", async () => {
    renderRail({ fail: true });
    await waitFor(() => expect(screen.getByTestId("assistant-rail")).toBeInTheDocument());
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(screen.queryByText("No conversations yet.")).not.toBeInTheDocument();
    const navLinks = screen.getByRole("navigation", { name: "Conversations" }).querySelectorAll("a");
    expect(navLinks).toHaveLength(0);
  });
});
