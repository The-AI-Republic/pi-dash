// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import type { Project, ProjectDetail } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { authKeys } from "../../auth/index.js";
import { projectKeys } from "../../projects/index.js";
import { ArchivesDetailFrame } from "./ArchivesDetailFrame.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const OTHER_ID = "323e4567-e89b-12d3-a456-426614174002";
const UUID = "123e4567-e89b-12d3-a456-426614174000";

function detailRow(overrides: Partial<ProjectDetail> = {}): ProjectDetail {
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
    ...overrides,
  };
}

function listRow(overrides: Partial<Project> = {}): Project {
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
    ...overrides,
  };
}

function renderFrame(detailCrumb: string | undefined, seedRows: boolean, narrow = false) {
  vi.stubGlobal("matchMedia", () => ({
    matches: narrow,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (seedRows) {
    queryClient.setQueryData(authKeys.project("acme", PROJECT_ID), detailRow());
    queryClient.setQueryData(projectKeys.list("acme"), [listRow(), listRow({ id: OTHER_ID, name: "Mobile" })]);
  }
  const rootRoute = createRootRoute({
    component: () => (
      <QueryClientProvider client={queryClient}>
        <ArchivesDetailFrame workspaceSlug="acme" projectId={PROJECT_ID} detailCrumb={detailCrumb}>
          <p>detail body</p>
        </ArchivesDetailFrame>
      </QueryClientProvider>
    ),
  });
  const homeRoute = createRoute({ getParentRoute: () => rootRoute, path: "/", component: () => null });
  const router = createRouter({
    routeTree: rootRoute.addChildren([homeRoute]),
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
  render(<RouterProvider router={router} />);
  return router;
}

afterEach(() => {
  resetAppClient();
  vi.unstubAllGlobals();
});

describe("ArchivesDetailFrame", () => {
  it("frames the detail without the tab strip or badge", async () => {
    renderFrame(undefined, true);

    expect(await screen.findByTestId("archives-detail-frame")).not.toBeNull();
    const crumbs = screen.getByRole("navigation", { name: "Breadcrumb" }).querySelectorAll("li");
    expect(crumbs).toHaveLength(2);
    expect(screen.queryByTestId("archives-tab-strip")).toBeNull();
    expect(screen.queryByTestId("archives-count-badge")).toBeNull();
    expect(screen.getByTestId("archives-content")).toHaveTextContent("detail body");
  });

  it("ends the trail at the detail crumb once named", async () => {
    renderFrame("WEB-12", true);

    const crumbs = (await screen.findByRole("navigation", { name: "Breadcrumb" })).querySelectorAll("li");
    expect(crumbs).toHaveLength(3);
    expect(crumbs[2]?.textContent).toBe("WEB-12");
  });

  it("renders nothing while the project row resolves", async () => {
    const router = renderFrame(undefined, false);

    await waitFor(() => expect(router.state.isLoading).toBe(false));
    expect(screen.queryByTestId("archives-detail-frame")).toBeNull();
  });

  it("switches projects at the work-items tab", async () => {
    const router = renderFrame(undefined, true);

    fireEvent.click(await screen.findByTestId("project-switcher-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Mobile" }));

    await waitFor(() => expect(router.state.location.pathname).toBe(`/acme/projects/${OTHER_ID}/archives/issues`));
  });

  it("stays put when the switcher picks the current project", async () => {
    const router = renderFrame(undefined, true);

    fireEvent.click(await screen.findByTestId("project-switcher-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Web" }));

    expect(router.state.location.pathname).toBe("/");
    expect(await screen.findByTestId("archives-detail-frame")).not.toBeNull();
  });

  it("backs out through history on narrow viewports", async () => {
    const router = renderFrame(undefined, true, true);
    const back = vi.spyOn(router.history, "back");

    fireEvent.click(await screen.findByTestId("archives-back"));
    expect(back).toHaveBeenCalledTimes(1);
  });
});
