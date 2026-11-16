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
import { archivesKeys } from "../api/keys.js";
import type { ArchivesTabKey } from "../tabs.js";
import { ArchivesShell } from "./ArchivesShell.js";

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

interface ShellSeed {
  detail?: ProjectDetail | undefined;
  list?: Project[] | undefined;
  count?: number | undefined;
}

function renderShell(activeTab: ArchivesTabKey, seed: ShellSeed = {}, narrow = false) {
  vi.stubGlobal("matchMedia", () => ({
    matches: narrow,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (seed.detail !== undefined) queryClient.setQueryData(authKeys.project("acme", PROJECT_ID), seed.detail);
  if (seed.list !== undefined) queryClient.setQueryData(projectKeys.list("acme"), seed.list);
  if (seed.count !== undefined) queryClient.setQueryData(archivesKeys.count("acme", PROJECT_ID), seed.count);
  const rootRoute = createRootRoute({
    component: () => (
      <QueryClientProvider client={queryClient}>
        <ArchivesShell workspaceSlug="acme" projectId={PROJECT_ID} activeTab={activeTab}>
          <p>tab body</p>
        </ArchivesShell>
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

const FULL_SEED: ShellSeed = {
  detail: detailRow(),
  list: [listRow(), listRow({ id: OTHER_ID, name: "Mobile" })],
  count: 2,
};

afterEach(() => {
  resetAppClient();
  vi.unstubAllGlobals();
});

describe("ArchivesShell", () => {
  it("renders the header, strip and tab slot on the work-items tab", async () => {
    renderShell("issues", FULL_SEED);

    expect(await screen.findByTestId("archives-shell")).not.toBeNull();
    const crumbs = screen.getByRole("navigation", { name: "Breadcrumb" }).querySelectorAll("li");
    expect(crumbs).toHaveLength(3);
    expect(crumbs[2]?.textContent).toBe("Work items");
    expect(screen.getByRole("link", { name: "Cycles" })).not.toBeNull();
    expect(screen.getByRole("link", { name: "Modules" })).not.toBeNull();
    expect(screen.getByRole("link", { name: "Work items" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByTestId("archives-count-badge")).toHaveTextContent("2");
    expect(screen.getByTestId("archives-content")).toHaveTextContent("tab body");
  });

  it("gates the strip by the project flags and drops the badge off the issues tab", async () => {
    renderShell("cycles", {
      detail: detailRow({ cycle_view: false, module_view: false }),
      list: [listRow()],
      count: 2,
    });

    await screen.findByRole("link", { name: "Work items" });
    expect(screen.queryByRole("link", { name: "Cycles" })).toBeNull();
    expect(screen.queryByRole("link", { name: "Modules" })).toBeNull();
    expect(screen.getByRole("link", { name: "Work items" })).not.toBeNull();
    expect(screen.queryByTestId("archives-count-badge")).toBeNull();
  });

  it("renders nothing while the project row resolves", async () => {
    const router = renderShell("issues", { list: [listRow()], count: 2 });

    await waitFor(() => expect(router.state.isLoading).toBe(false));
    expect(screen.queryByTestId("archives-shell")).toBeNull();
  });

  it("switches projects at the same archives tab", async () => {
    const router = renderShell("cycles", FULL_SEED);

    fireEvent.click(await screen.findByTestId("project-switcher-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Mobile" }));

    await waitFor(() => expect(router.state.location.pathname).toBe(`/acme/projects/${OTHER_ID}/archives/cycles`));
  });

  it("stays put when the switcher picks the current project", async () => {
    const router = renderShell("cycles", FULL_SEED);

    fireEvent.click(await screen.findByTestId("project-switcher-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Web" }));

    expect(router.state.location.pathname).toBe("/");
    expect(await screen.findByTestId("archives-shell")).not.toBeNull();
  });

  it("backs out through history on narrow viewports", async () => {
    const router = renderShell("issues", FULL_SEED, true);
    const back = vi.spyOn(router.history, "back");

    fireEvent.click(await screen.findByTestId("archives-back"));
    expect(back).toHaveBeenCalledTimes(1);
  });
});
