// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Test harness for the workspace-settings route files. The `-` prefix
// keeps it out of the generated route tree. It rebuilds the same parent
// ids the app uses (`/$ws` -> `_gated` -> `settings/_workspace`), so the
// real route files' `Route.useParams()` resolves against the mounted
// tree instead of a test-only copy.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
  type NotFoundRouteComponent,
  type RouteComponent,
} from "@tanstack/react-router";
import { render } from "@testing-library/react";
import * as React from "react";

import { createAppClient, resetAppClient } from "../../../../../core/api/client.js";
import type { Transport } from "@pidash/api-client";
import type { RouterContext } from "../../../../__root.js";

export interface RouteHarnessOptions {
  /** Real layout component; the settings layout renders a passthrough by default. */
  layoutComponent?: RouteComponent;
  /** The leaf under test. */
  component: RouteComponent;
  /** Leaf path relative to the settings layout ("/" for the index). */
  leafPath: string;
  /** Initial address; defaults to the address the leaf models. */
  initialPath?: string;
  /** Whether the layout runs (the parent `_gated` waterfall tests this). */
  gatedLayout?: boolean;
  transport: Transport;
}

export function renderSettingsRoute({
  layoutComponent,
  component,
  leafPath,
  initialPath,
  gatedLayout = true,
  transport,
}: RouteHarnessOptions) {
  const apiClient = createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const notFoundComponent: NotFoundRouteComponent = () => <p>Not found in harness</p>;
  const rootRoute = createRootRouteWithContext<RouterContext>()({ notFoundComponent });
  const wsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/$ws", component: () => <Outlet /> });
  const gatedRoute = createRoute({
    getParentRoute: () => wsRoute,
    id: "_gated",
    ...(gatedLayout ? { beforeLoad: () => ({ gated: true as const }) } : {}),
    component: () => <Outlet />,
  });
  const settingsRoute = createRoute({
    getParentRoute: () => gatedRoute,
    path: "settings",
    component: () => <Outlet />,
  });
  const layoutRoute = createRoute({
    getParentRoute: () => settingsRoute,
    id: "_workspace",
    component: layoutComponent ?? (() => <Outlet />),
  });
  // `settings` is a plain path segment, not a pathless id.
  const leafRoute = createRoute({
    getParentRoute: () => layoutRoute,
    // The layout file serves the section addresses from a child directory,
    // so the leaf is mounted one level down under a pathless parent.
    // `$` is the catch-all leaf, so pass it through untouched.
    path: leafPath === "/" || leafPath === "$" ? leafPath : `/${leafPath.replace(/^\//, "")}`,
    component,
  });
  const leafParentRoute = createRoute({
    getParentRoute: () => layoutRoute,
    id: "_leaf",
    component: () => <Outlet />,
  });
  leafParentRoute.addChildren([leafRoute]);
  const leafAddress = leafPath === "/" ? "/acme/settings/" : `/acme/settings/${leafPath.replace(/\/$/, "")}/`;
  const router = createRouter({
    routeTree: rootRoute.addChildren([
      wsRoute.addChildren([
        gatedRoute.addChildren([settingsRoute.addChildren([layoutRoute.addChildren([leafParentRoute])])]),
      ]),
    ]),
    context: { queryClient, apiClient },
    history: createMemoryHistory({ initialEntries: [initialPath ?? leafAddress] }),
  });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { router, queryClient, apiClient, ...view };
}

export function resetRouteHarness(): void {
  resetAppClient();
}

/** Stub transport answering the reads the settings frame makes. */
export function settingsTransport(options: {
  role: number;
  workspaceList?: unknown[];
  detailsStatus?: number;
  detailsName?: string;
  detailsLogoUrl?: string | null;
  listedLogoUrl?: string | null;
  workspacesPending?: boolean;
  onRequest?: (url: string) => void;
}): Transport {
  return async (url) => {
    options.onRequest?.(url);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return ok(url, { csrf_token: "csrf" });
    }
    if (url.endsWith("/api/users/me/workspaces/")) {
      if (options.workspacesPending === true) return new Promise(() => undefined);
      return ok(
        url,
        options.workspaceList ?? [
          {
            id: "123e4567-e89b-12d3-a456-426614174000",
            name: "Acme Listed",
            slug: "acme",
            logo_url: options.listedLogoUrl ?? null,
            total_members: 3,
            role: options.role,
          },
        ]
      );
    }
    if (url.endsWith("/api/workspaces/acme/")) {
      const status = options.detailsStatus ?? 200;
      if (status !== 200) return { status, url, headers: { get: () => null }, text: async () => "{}" };
      return ok(url, {
        id: "123e4567-e89b-12d3-a456-426614174000",
        name: options.detailsName ?? "Acme",
        slug: "acme",
        logo: null,
        logo_asset: null,
        logo_url: options.detailsLogoUrl ?? null,
        owner: "123e4567-e89b-12d3-a456-426614174000",
        organization_size: null,
        timezone: "UTC",
        background_color: "#ffffff",
        total_members: 3,
        created_by: null,
        updated_by: null,
        created_at: "2024-01-02T03:04:05Z",
        updated_at: "2024-02-03T04:05:06Z",
      });
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  };
}

function ok(url: string, body: unknown) {
  return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
}
