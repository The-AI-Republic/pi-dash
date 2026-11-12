// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Test harness for the profile route files. The `-` prefix keeps it out of
// the generated route tree. It rebuilds the parent ids the app uses, so
// the real route files' `Route.useParams()` resolves against the mounted
// tree instead of a test-only copy.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
  type RouteComponent,
} from "@tanstack/react-router";
import { render } from "@testing-library/react";
import * as React from "react";

import { createAppClient } from "../../../../../core/api/client.js";

export interface ProfileRouteHarnessOptions {
  component?: RouteComponent;
  /** "/" for the summary leaf, otherwise the tab segment. */
  leafPath: string;
  initialPath?: string;
  /** Seeds query data for the screens under test. */
  seed?: (queryClient: QueryClient) => void;
}

export function renderProfileRoute({ component, leafPath, initialPath, seed }: ProfileRouteHarnessOptions) {
  const apiClient = createAppClient("https://api.test", () => new Promise<never>(() => {}));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  seed?.(queryClient);
  const rootRoute = createRootRouteWithContext<{ queryClient: QueryClient; apiClient: typeof apiClient }>()({});
  const wsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/$ws", component: () => <Outlet /> });
  const gatedRoute = createRoute({ getParentRoute: () => wsRoute, id: "_gated", component: () => <Outlet /> });
  const profileRoute = createRoute({
    getParentRoute: () => gatedRoute,
    path: "profile/$userId",
    component: () => <Outlet />,
  });
  const leafRoute = createRoute({
    getParentRoute: () => profileRoute,
    path: leafPath,
    ...(component === undefined ? {} : { component }),
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([
      wsRoute.addChildren([gatedRoute.addChildren([profileRoute.addChildren([leafRoute])])]),
    ]),
    context: { queryClient, apiClient },
    history: createMemoryHistory({
      initialEntries: [initialPath ?? `/acme/profile/u-1/${leafPath === "/" ? "" : leafPath}`],
    }),
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { queryClient, router };
}
