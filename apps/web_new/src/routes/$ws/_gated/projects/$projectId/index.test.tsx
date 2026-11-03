// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bare project address (SHELL-077). The address hands its path params to
// the default redirect, which rewrites to the member's stored default
// tab; the redirect's own fallbacks belong to its suite.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../../../core/api/client.js";
import type { RouterContext } from "../../../../__root.js";
import { Route } from "./index.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const UUID = "123e4567-e89b-12d3-a456-426614174000";

function projectRow() {
  return {
    id: PROJECT_ID,
    name: "Web",
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    archived_at: null,
    workspace: UUID,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: true,
    inbox_view: true,
    is_default: true,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
  };
}

function renderBareAddress(): void {
  const apiClient = createAppClient("https://api.test", async (url) => {
    let status = 404;
    let body = "{}";
    if (url.endsWith("/auth/get-csrf-token/")) {
      status = 200;
      body = JSON.stringify({ csrf_token: "csrf" });
    } else if (url.includes("/project-roles/")) {
      status = 200;
      body = JSON.stringify({ [PROJECT_ID]: 20 });
    } else if (url.includes(`/projects/${PROJECT_ID}/user-properties/`)) {
      status = 200;
      body = JSON.stringify({
        id: UUID,
        workspace: UUID,
        project: PROJECT_ID,
        user: UUID,
        filters: {},
        display_filters: {},
        display_properties: {},
        rich_filters: {},
        preferences: { navigation: { default_tab: "cycles", hide_in_more_menu: [] } },
        sort_order: 1,
        created_by: null,
        updated_by: null,
        created_at: "2024-01-02T03:04:05Z",
        updated_at: "2024-02-03T04:05:06Z",
      });
    } else if (url.endsWith("/projects/")) {
      status = 200;
      body = JSON.stringify([{ ...projectRow(), intake_count: 0 }]);
    }
    return { status, url, headers: { get: () => null }, text: async () => body };
  });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const component = Route.options.component;
  if (!component) throw new Error("missing component");
  const rootRoute = createRootRouteWithContext<RouterContext>()();
  const wsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/$ws",
    component: () => <Outlet />,
  });
  const gatedRoute = createRoute({
    getParentRoute: () => wsRoute,
    id: "_gated",
    component: () => <Outlet />,
  });
  const projectRoute = createRoute({
    getParentRoute: () => gatedRoute,
    path: "projects/$projectId",
    component: () => <Outlet />,
  });
  const bareRoute = createRoute({
    getParentRoute: () => projectRoute,
    path: "/",
    component,
  });
  const cyclesRoute = createRoute({
    getParentRoute: () => projectRoute,
    path: "cycles",
    component: () => <p>Cycles page</p>,
  });
  const issuesRoute = createRoute({
    getParentRoute: () => projectRoute,
    path: "issues",
    component: () => <p>Issues page</p>,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([
      wsRoute.addChildren([gatedRoute.addChildren([projectRoute.addChildren([bareRoute, cyclesRoute, issuesRoute])])]),
    ]),
    context: { queryClient, apiClient },
    history: createMemoryHistory({ initialEntries: [`/acme/projects/${PROJECT_ID}/`] }),
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
});

describe("bare project route", () => {
  it("rewrites the bare address to the stored default tab", async () => {
    renderBareAddress();
    await screen.findByText("Cycles page");
    expect(screen.queryByText("Issues page")).not.toBeInTheDocument();
  });
});
