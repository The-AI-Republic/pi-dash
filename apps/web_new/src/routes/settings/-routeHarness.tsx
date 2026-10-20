// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Test harness for the profile-settings route files. The `-` prefix keeps
// it out of the generated route tree. The layout's own path is the real
// `/settings/profile`, so the real route file's hooks resolve against the
// mounted tree.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  RouterProvider,
  type RouteComponent,
} from "@tanstack/react-router";
import { render } from "@testing-library/react";
import * as React from "react";

import type { Transport } from "@pidash/api-client";
import { createAppClient, resetAppClient } from "../../core/api/client.js";
import type { RouterContext } from "../__root.js";

export function renderProfileSettingsRoute(args: {
  layoutComponent: RouteComponent;
  leafComponent: RouteComponent;
  leafPath: string;
  initialPath: string;
  transport: Transport;
  /** Loader answers the layout component reads from its route context. */
  routeContext?: Record<string, unknown>;
}) {
  const apiClient = createAppClient("https://api.test", args.transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const rootRoute = createRootRouteWithContext<RouterContext>()({});
  const layoutRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/settings/profile",
    component: args.layoutComponent,
  });
  const leafRoute = createRoute({
    getParentRoute: () => layoutRoute,
    path: args.leafPath,
    component: args.leafComponent,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([layoutRoute.addChildren([leafRoute])]),
    context: { queryClient, apiClient, ...(args.routeContext ?? {}) },
    history: createMemoryHistory({ initialEntries: [args.initialPath] }),
  });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { router, queryClient, ...view };
}

export function resetProfileRouteHarness(): void {
  resetAppClient();
}
