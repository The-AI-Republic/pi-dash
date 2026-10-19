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
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { RouterContext } from "../../../__root.js";
import { Route } from "./$threadId.js";

vi.mock("../../../../features/assistant/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../../features/assistant/index.js")>();
  return {
    ...actual,
    ConversationGate: (props: { workspaceSlug: string; threadId: string; state: unknown }) => (
      <div data-testid="gate-slot" data-slug={props.workspaceSlug} data-thread={props.threadId}>
        {String((props.state as { pendingMessage?: unknown } | null)?.pendingMessage ?? "")}
      </div>
    ),
  };
});

describe("conversation route", () => {
  it("passes the params and the handoff state to the gate", async () => {
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
});
