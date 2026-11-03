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
import type { ArchivedCycleListItem, ProjectDetail } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../../../../core/api/client.js";
import { authKeys } from "../../../../../../../features/auth/index.js";
import { cycleKeys } from "../../../../../../../features/cycles/index.js";
import { makeCycleRow } from "../../../../../../../features/cycles/testing.js";
import { Route } from "./index.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const UUID = "123e4567-e89b-12d3-a456-426614174000";
const ROW_ID = "323e4567-e89b-12d3-a456-426614174101";

function archivedRow(name: string): ArchivedCycleListItem {
  return {
    ...makeCycleRow({ id: ROW_ID, name }),
    start_date: "2026-01-01T00:00:00Z",
    end_date: "2026-02-01T00:00:00Z",
    status: "completed",
    description: "",
    owned_by_id: UUID,
    archived_at: "2026-03-01T00:00:00Z",
    total_issues: 1,
    completed_issues: 1,
    started_issues: 0,
    unstarted_issues: 0,
    cancelled_issues: 0,
    backlog_issues: 0,
    assignee_ids: [],
  };
}

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

function renderTab(
  row: ProjectDetail | undefined,
  search: Record<string, unknown> = {},
  archived: ArchivedCycleListItem[] | undefined = undefined
) {
  document.body.innerHTML = `<div id="full-screen-portal"></div>`;
  vi.stubGlobal("matchMedia", () => ({
    matches: false,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: PROJECT_ID });
  vi.spyOn(Route, "useSearch").mockReturnValue(search);
  const Component = Route.options.component;
  if (!Component) throw new Error("archived cycles tab has no component");
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (row !== undefined) queryClient.setQueryData(authKeys.project("acme", PROJECT_ID), row);
  if (archived !== undefined) queryClient.setQueryData(cycleKeys.archived("acme", PROJECT_ID), archived);
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
  return router;
}

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("archived cycles tab route", () => {
  it("mounts the shell with the active tab and per-tab title", async () => {
    renderTab(detailRow());

    await screen.findByTestId("archives-shell");
    expect(screen.getByRole("link", { name: "Cycles" })).toHaveAttribute("aria-current", "page");
    await waitFor(() => expect(document.title).toBe("Web - Archived cycles"));
  });

  it("mounts the archived-cycles screen in the shell slot", async () => {
    renderTab(detailRow());

    await screen.findByTestId("archives-shell");
    expect(await screen.findByTestId("archived-cycles-screen")).toBeInTheDocument();
    expect(screen.queryByTestId("archived-cycle-preview")).toBeNull();
  });

  it("opens the peeked cycle from the address key", async () => {
    renderTab(detailRow(), { peekCycle: UUID });

    await screen.findByTestId("archives-shell");
    expect(await screen.findByTestId("archived-cycle-preview")).toBeInTheDocument();
  });

  it("awaits the screen chunk before the route commits", async () => {
    const beforeLoad = Route.options.beforeLoad;
    if (typeof beforeLoad !== "function") throw new Error("archived cycles tab has no chunk prefetch");
    await expect(beforeLoad({} as never)).resolves.toBeUndefined();
  });

  it("validates the peek key and drops anything invalid", () => {
    const validate = Route.options.validateSearch;
    if (typeof validate !== "function") throw new Error("archived cycles tab has no search validation");
    expect(validate({ peekCycle: UUID })).toEqual({ peekCycle: UUID });
    expect(validate({})).toEqual({});
    expect(validate({ peekCycle: 42 })).toEqual({});
  });

  it("writes the peek key when a row's quick look opens", async () => {
    const router = renderTab(detailRow(), {}, [archivedRow("Archived alpha")]);

    fireEvent.click(await screen.findByRole("button", { name: "Quick look at Archived alpha" }));

    await waitFor(() => expect(router.state.location.search).toEqual({ peekCycle: ROW_ID }));
  });

  it("clears the peek key when the peeked row's quick look closes", async () => {
    const router = renderTab(detailRow(), { peekCycle: ROW_ID }, [archivedRow("Archived alpha")]);

    fireEvent.click(await screen.findByRole("button", { name: "Quick look at Archived alpha" }));

    await waitFor(() => expect(router.state.location.search).toEqual({}));
  });

  it("lands on the work-items tab when the cycles flag is off", async () => {
    const router = renderTab(detailRow({ cycle_view: false }));

    await waitFor(() => expect(router.state.location.pathname).toBe(`/acme/projects/${PROJECT_ID}/archives/issues`));
    expect(screen.queryByTestId("archives-shell")).toBeNull();
  });

  it("holds the fallback title while the project row resolves", async () => {
    const router = renderTab(undefined);

    await waitFor(() => expect(router.state.isLoading).toBe(false));
    expect(document.title).toBe("Archives");
    expect(screen.queryByTestId("archives-shell")).toBeNull();
  });
});
