// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant landing route (NEWFRONT-603): params reach the feature.
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
import { Route } from "./index.js";

vi.mock("../../../../features/assistant/components/AssistantLanding.js", () => ({
  AssistantLanding: (props: { workspaceSlug: string }) => <div data-testid="landing-slot">{props.workspaceSlug}</div>,
}));

describe("assistant landing route", () => {
  it("renders the feature landing for the workspace param", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const component = Route.options.component;
    if (component === undefined) throw new Error("missing component");
    const rootRoute = createRootRouteWithContext<RouterContext>()();
    const wsRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/$ws",
      component: () => <Outlet />,
    });
    const landingRoute = createRoute({
      getParentRoute: () => wsRoute,
      path: "/assistant",
      component,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([wsRoute.addChildren([landingRoute])]),
      context: { queryClient, apiClient: {} as never },
      history: createMemoryHistory({ initialEntries: ["/acme/assistant"] }),
    });
    render(
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    );
    expect(await screen.findByTestId("landing-slot")).toHaveTextContent("acme");
  });
});
