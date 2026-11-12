// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant layout route (NEWFRONT-603): the loader warms the thread list
// and tolerates a refused read (guests); the component mounts the layout.
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
import { assistantKeys } from "../../../../features/assistant/index.js";
import { Route } from "./route.js";

vi.mock("../../../../features/assistant/components/AssistantLayout.js", () => ({
  AssistantLayout: (props: { workspaceSlug: string }) => <div data-testid="layout-slot">{props.workspaceSlug}</div>,
}));

const THREAD = {
  id: "11111111-1111-4111-8111-111111111111",
  title: "",
  is_archived: false,
  has_active_turn: false,
  created_at: "2026-10-01T00:00:00Z",
  updated_at: "2026-10-02T00:00:00Z",
};

function stubClient(body: (url: string) => unknown) {
  return {
    getJson: async (url: string) => body(url),
    parse: (_schema: unknown, data: unknown) => data,
  } as never;
}

describe("assistant layout route", () => {
  it("warms the thread list in its loader", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const apiClient = stubClient((url) => {
      if (url.endsWith("/ai-assistant/threads/")) return [THREAD];
      throw new Error(`unexpected ${url}`);
    });
    const loader = Route.options.loader;
    if (typeof loader !== "function") throw new Error("missing loader");
    await loader({ context: { queryClient, apiClient }, params: { ws: "acme" } } as never);
    expect(queryClient.getQueryData(assistantKeys.threads("acme"))).toEqual([THREAD]);
  });

  it("tolerates a refused thread list (guests)", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const apiClient = stubClient(() => {
      throw new Error("role_not_allowed");
    });
    const loader = Route.options.loader;
    if (typeof loader !== "function") throw new Error("missing loader");
    await expect(
      loader({ context: { queryClient, apiClient }, params: { ws: "acme" } } as never)
    ).resolves.toBeUndefined();
    expect(queryClient.getQueryData(assistantKeys.threads("acme"))).toBeUndefined();
  });

  it("mounts the assistant layout for the workspace param", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const component = Route.options.component;
    if (component === undefined) throw new Error("missing component");
    const rootRoute = createRootRouteWithContext<RouterContext>()();
    const wsRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/$ws",
      component: () => <Outlet />,
    });
    const layoutRoute = createRoute({
      getParentRoute: () => wsRoute,
      path: "/assistant",
      component,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([wsRoute.addChildren([layoutRoute])]),
      context: { queryClient, apiClient: {} as never },
      history: createMemoryHistory({ initialEntries: ["/acme/assistant"] }),
    });
    render(
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    );
    expect(await screen.findByTestId("layout-slot")).toHaveTextContent("acme");
  });
});
