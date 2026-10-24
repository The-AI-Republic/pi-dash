// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project layout wiring (AUTH-024). The layout hands its path params to
// the gate and renders the page through it; the gate's screens belong to
// the auth area, so this suite only proves the params flow and the
// Outlet passthrough.
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
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../../core/api/client.js";
import type { RouterContext } from "../../../../__root.js";
import { Route } from "./route.js";

// The views palette entries load on their own chunk. The import never
// settles here so every render below sees the layout with the chunk still
// in flight: the page must not wait for it (NEWFRONT-871, size gate).
vi.mock("../../../../../features/views/palette/ViewsPaletteEntries.js", async () => {
  await new Promise(() => undefined);
  return {};
});

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
    cycle_view: false,
    issue_views_view: false,
    module_view: false,
    page_view: true,
    inbox_view: false,
    is_default: true,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    agent_executor_options: [
      { kind: "cloud_agent", available: false, reason_code: "cloud_agent_unavailable" },
      { kind: "local_runner", available: true, reason_code: "" },
      { kind: "managed_runner", available: false, reason_code: "managed_runner_disabled" },
    ],
    default_agent_executor: "local_runner",
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
  };
}

function membershipRow(role: number) {
  return {
    id: UUID,
    workspace: UUID,
    member: UUID,
    role,
    company_role: null,
    view_props: {},
    default_props: {},
    issue_props: {},
    is_active: true,
    getting_started_checklist: {},
    tips: {},
    explored_features: {},
    draft_issue_count: 0,
    created_by: null,
    updated_by: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
  };
}

function userPropertiesPayload(preference: string) {
  return {
    id: UUID,
    workspace: UUID,
    user: UUID,
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
    navigation_project_limit: 10,
    navigation_control_preference: preference,
    created_by: null,
    updated_by: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
  };
}

function renderLayout(detailStatus: number): void {
  const apiClient = createAppClient("https://api.test", async (url) => {
    let status = 404;
    let body = "{}";
    if (url.endsWith("/auth/get-csrf-token/")) {
      status = 200;
      body = JSON.stringify({ csrf_token: "csrf" });
    } else if (url.includes("/project-roles/")) {
      status = 200;
      body = JSON.stringify({ [PROJECT_ID]: 20 });
    } else if (url.endsWith(`/projects/${PROJECT_ID}/`)) {
      status = detailStatus;
      body = detailStatus === 200 ? JSON.stringify(projectRow()) : JSON.stringify({ error: "denied" });
    } else if (url.includes(`/projects/${PROJECT_ID}/user-properties/`)) {
      status = 200;
      body = JSON.stringify({
        ...userPropertiesPayload("TABBED"),
        project: PROJECT_ID,
        preferences: { navigation: { default_tab: "work_items", hide_in_more_menu: [] } },
        sort_order: 1,
      });
    } else if (url.endsWith("/user-properties/")) {
      status = 200;
      body = JSON.stringify(userPropertiesPayload("TABBED"));
    } else if (url.endsWith("/projects/")) {
      status = 200;
      body = JSON.stringify([{ ...projectRow(), intake_count: 0 }]);
    } else if (url.endsWith("/workspace-members/me/")) {
      status = 200;
      body = JSON.stringify(membershipRow(15));
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
  const layoutRoute = createRoute({
    getParentRoute: () => gatedRoute,
    path: "projects/$projectId",
    component,
  });
  const leafRoute = createRoute({
    getParentRoute: () => layoutRoute,
    path: "issues",
    component: () => <p>Issues page</p>,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([
      wsRoute.addChildren([gatedRoute.addChildren([layoutRoute.addChildren([leafRoute])])]),
    ]),
    context: { queryClient, apiClient },
    history: createMemoryHistory({ initialEntries: [`/acme/projects/${PROJECT_ID}/issues`] }),
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

describe("project layout", () => {
  it("renders the tab strip and page while the views palette chunk is still loading", async () => {
    renderLayout(200);
    await screen.findByTestId("project-tab-strip");
    expect(screen.getByText("Issues page")).toBeInTheDocument();
  });

  it("renders the page through the gate for members", async () => {
    renderLayout(200);
    await screen.findByText("Issues page");
  });

  it("renders the tab strip above the page for members", async () => {
    renderLayout(200);
    await screen.findByTestId("project-tab-strip");
    expect(screen.getByTestId("project-switcher-trigger")).toHaveTextContent("Web");
    expect(screen.getByRole("link", { name: "Work Items" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByText("Issues page")).toBeInTheDocument();
  });

  it("renders the denial screen for refused projects", async () => {
    renderLayout(409);
    await screen.findByRole("heading", { name: "You haven't joined this project yet" });
    expect(screen.queryByText("Issues page")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-tab-strip")).not.toBeInTheDocument();
  });

  it("renders not-found for unknown projects", async () => {
    renderLayout(404);
    await screen.findByRole("heading", { name: "Project not found" });
  });
});
