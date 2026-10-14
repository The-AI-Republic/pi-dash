// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
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
import { afterEach, describe, expect, it } from "vitest";

import type { RouterContext } from "../../../__root.js";
import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { Route } from "./$issueKey.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE_ID = "323e4567-e89b-12d3-a456-426614174002";
const UUID = "123e4567-e89b-12d3-a456-426614174000";

function projectRow() {
  return {
    id: PROJECT_ID,
    name: "Web",
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
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

function issueRow() {
  return {
    id: ISSUE_ID,
    name: "Fix login",
    state_id: UUID,
    sort_order: 1.5,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    start_date: null,
    target_date: null,
    sequence_id: 1,
    project_id: PROJECT_ID,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: 0,
    attachment_count: null,
    link_count: 0,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p></p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
  };
}

function userPropertiesPayload() {
  return {
    id: UUID,
    workspace: UUID,
    user: UUID,
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
    navigation_project_limit: 10,
    navigation_control_preference: "TABBED",
    created_by: null,
    updated_by: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
  };
}

function renderBrowse(known: boolean, isEpic?: boolean): void {
  const apiClient = createAppClient("https://api.test", async (url) => {
    let status = 404;
    let body = "{}";
    if (url.endsWith("/auth/get-csrf-token/")) {
      status = 200;
      body = JSON.stringify({ csrf_token: "csrf" });
    } else if (url.includes("/search/")) {
      status = 200;
      body = JSON.stringify({
        results: {
          issue: known
            ? [
                {
                  name: "Fix login",
                  id: ISSUE_ID,
                  sequence_id: 1,
                  project__identifier: "WEB",
                  project_id: PROJECT_ID,
                  workspace__slug: "acme",
                },
              ]
            : [],
        },
      });
    } else if (url.includes("/project-roles/")) {
      status = 200;
      body = JSON.stringify({ [PROJECT_ID]: 20 });
    } else if (url.includes(`/projects/${PROJECT_ID}/user-properties/`)) {
      status = 200;
      body = JSON.stringify({
        ...userPropertiesPayload(),
        project: PROJECT_ID,
        preferences: { navigation: { default_tab: "work_items", hide_in_more_menu: [] } },
        sort_order: 1,
      });
    } else if (url.endsWith("/user-properties/")) {
      status = 200;
      body = JSON.stringify(userPropertiesPayload());
    } else if (url.endsWith("/projects/")) {
      status = 200;
      body = JSON.stringify([projectRow()]);
    } else if (url.includes(`/issues/${ISSUE_ID}/`)) {
      status = 200;
      body = JSON.stringify(isEpic === undefined ? issueRow() : { ...issueRow(), is_epic: isEpic });
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
  const browseRoute = createRoute({
    getParentRoute: () => gatedRoute,
    path: "browse/$issueKey",
    component,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([wsRoute.addChildren([gatedRoute.addChildren([browseRoute])])]),
    context: { queryClient, apiClient },
    history: createMemoryHistory({ initialEntries: ["/acme/browse/WEB-1"] }),
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

describe("browse route", () => {
  it("renders the detail under the project strip with tracking highlighted", async () => {
    renderBrowse(true);
    await screen.findByTestId("project-tab-strip");
    expect(screen.getByTestId("project-switcher-trigger")).toHaveTextContent("Web");
    expect(screen.getByRole("link", { name: "Work Items" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("heading", { name: "Fix login" })).toBeInTheDocument();
  });

  it("renders unknown keys without the strip", async () => {
    renderBrowse(false);
    await screen.findByRole("heading", { name: "No work item matches this address" });
    expect(screen.queryByTestId("project-tab-strip")).not.toBeInTheDocument();
  });

  it("highlights nothing for an epic while no epic tab renders", async () => {
    renderBrowse(true, true);
    await screen.findByTestId("project-tab-strip");
    expect(screen.getByRole("heading", { name: "Fix login" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Work Items" })).not.toHaveAttribute("aria-current");
    expect(document.querySelectorAll('[aria-current="page"]')).toHaveLength(0);
  });
});
