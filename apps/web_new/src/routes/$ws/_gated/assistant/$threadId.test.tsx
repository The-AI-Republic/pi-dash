// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation route (NEWFRONT-603): params plus the one-shot handoff
// state reach the conversation gate.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { RouterContext } from "../../../__root.js";
import { Route } from "./$threadId.js";

const localChat = vi.hoisted(() => ({ value: false }));

vi.mock("../../../../features/assistant/components/ConversationGate.js", () => ({
  ConversationGate: (props: { workspaceSlug: string; threadId: string; state: unknown }) => (
    <div data-testid="gate-slot" data-slug={props.workspaceSlug} data-thread={props.threadId}>
      {String((props.state as { pendingMessage?: unknown } | null)?.pendingMessage ?? "")}
    </div>
  ),
}));
vi.mock("./DesktopAssistant.js", () => ({
  DesktopAssistant: (props: {
    workspaceSlug: string;
    requestedSessionId: string | null;
    initialMessage?: string | null;
    onInitialMessageConsumed?: () => void;
  }) => (
    <button
      type="button"
      data-testid="desktop-slot"
      data-slug={props.workspaceSlug}
      data-session={props.requestedSessionId ?? ""}
      onClick={props.onInitialMessageConsumed}
    >
      {props.initialMessage}
    </button>
  ),
}));
vi.mock("../../../../core/platform/local-chat.js", () => ({ hasLocalChat: () => localChat.value }));

describe("conversation route", () => {
  it("passes the params and the handoff state to the gate without local chat", async () => {
    localChat.value = false;
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const component = Route.options.component;
    if (component === undefined) throw new Error("missing component");
    const rootRoute = createRootRouteWithContext<RouterContext>()();
    const wsRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/$ws",
      component: () => <Outlet />,
    });
    const conversationRoute = createRoute({
      getParentRoute: () => wsRoute,
      path: "/assistant/$threadId",
      component,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([wsRoute.addChildren([conversationRoute])]),
      context: { queryClient, apiClient: {} as never },
      history: createMemoryHistory({ initialEntries: ["/acme/assistant/thread-1"] }),
    });
    // The handoff state is attached the way the landing attaches it.
    await router.navigate({
      to: "/$ws/assistant/$threadId",
      params: { ws: "acme", threadId: "thread-1" },
      state: { pendingMessage: "hello" },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    );
    const gate = await screen.findByTestId("gate-slot");
    expect(gate).toHaveAttribute("data-slug", "acme");
    expect(gate).toHaveAttribute("data-thread", "thread-1");
    expect(gate).toHaveTextContent("hello");
  });

  it("renders the desktop assistant for the thread id when local chat is present", async () => {
    localChat.value = true;
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const component = Route.options.component;
    if (component === undefined) throw new Error("missing component");
    const rootRoute = createRootRouteWithContext<RouterContext>()();
    const wsRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/$ws",
      component: () => <Outlet />,
    });
    const conversationRoute = createRoute({
      getParentRoute: () => wsRoute,
      path: "/assistant/$threadId",
      component,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([wsRoute.addChildren([conversationRoute])]),
      context: { queryClient, apiClient: {} as never },
      history: createMemoryHistory({ initialEntries: ["/acme/assistant/sess-1"] }),
    });
    render(
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    );
    await router.navigate({ href: "/acme/assistant/sess-1", state: { pendingMessage: "hello native" } });
    const slot = await screen.findByTestId("desktop-slot");
    expect(slot).toHaveAttribute("data-slug", "acme");
    expect(slot).toHaveAttribute("data-session", "sess-1");
    await waitFor(() => expect(slot).toHaveTextContent("hello native"));
    fireEvent.click(slot);
    await waitFor(() =>
      expect((router.state.location.state as { pendingMessage?: string }).pendingMessage).toBeUndefined()
    );
  });
});
