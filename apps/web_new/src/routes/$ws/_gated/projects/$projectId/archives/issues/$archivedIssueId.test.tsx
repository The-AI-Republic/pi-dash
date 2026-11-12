// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import type { ProjectDetail } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../../../../core/api/client.js";
import { authKeys } from "../../../../../../../features/auth/index.js";
import { Route } from "./$archivedIssueId.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const UUID = "123e4567-e89b-12d3-a456-426614174000";

function detailRow(): ProjectDetail {
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
    issue_views_view: false,
    module_view: true,
    page_view: true,
    inbox_view: false,
    is_default: true,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    agent_executor_options: [],
    default_agent_executor: "local_runner",
  };
}

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("archived issue detail route", () => {
  it("mounts the detail frame without the tab strip", async () => {
    vi.stubGlobal("matchMedia", () => ({
      matches: false,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
    }));
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: PROJECT_ID, archivedIssueId: UUID });
    const Component = Route.options.component;
    if (!Component) throw new Error("archived issue detail has no component");
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(authKeys.project("acme", PROJECT_ID), detailRow());
    const rootRoute = createRootRoute({
      component: () => (
        <QueryClientProvider client={queryClient}>
          <Component />
        </QueryClientProvider>
      ),
    });
    const homeRoute = createRoute({ getParentRoute: () => rootRoute, path: "/", component: () => null });
    const router = createRouter({
      routeTree: rootRoute.addChildren([homeRoute]),
      history: createMemoryHistory({ initialEntries: ["/"] }),
    });
    render(<RouterProvider router={router} />);

    await screen.findByTestId("archives-detail-frame");
    expect(screen.queryByTestId("archives-tab-strip")).toBeNull();
    expect(screen.getByTestId("archives-content")).not.toBeNull();
  });
});
