// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
const navigate = vi.hoisted(() => vi.fn());
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNavigate: () => navigate,
}));

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import type { ProjectDetail } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../../../../core/api/client.js";
import { authKeys } from "../../../../../../../features/auth/index.js";
import { makeModuleRow } from "../../../../../../../features/modules/testing.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../../../../../shared/shell/document.js";
import { Route } from "./index.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const UUID = "123e4567-e89b-12d3-a456-426614174000";
const ARCHIVED_ID = "323e4567-e89b-12d3-a456-426614174201";

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

function archivedRow(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    ...makeModuleRow({ name: "Archived alpha", status: "completed" }),
    description_text: null,
    description_html: null,
    archived_at: "2026-03-01T00:00:00Z",
    ...overrides,
  };
}

function renderTab(
  row: ProjectDetail | undefined,
  search: Record<string, unknown> = {},
  archived: Record<string, unknown>[] = []
) {
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  vi.stubGlobal("matchMedia", () => ({
    matches: false,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: PROJECT_ID });
  vi.spyOn(Route, "useSearch").mockReturnValue(search as never);
  const Component = Route.options.component;
  if (!Component) throw new Error("archived modules tab has no component");
  const archivedUrl = `/api/workspaces/acme/projects/${PROJECT_ID}/archived-modules/`;
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith(archivedUrl)) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(archived),
      };
    }
    return new Promise<never>(() => {});
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (row !== undefined) queryClient.setQueryData(authKeys.project("acme", PROJECT_ID), row);
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const tabPath = `/acme/projects/${PROJECT_ID}/archives/modules`;
  const tabRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: tabPath,
    validateSearch: (search: Record<string, unknown>) => search,
    component: () => (
      <QueryClientProvider client={queryClient}>
        <Component />
      </QueryClientProvider>
    ),
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([tabRoute]),
    history: createMemoryHistory({ initialEntries: [tabPath] }),
    defaultPreload: false,
  });
  render(<RouterProvider router={router} />);
  return router;
}

afterEach(() => {
  navigate.mockReset();
  resetAppClient();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("archived modules tab route", () => {
  it("keeps a usable peek key and drops an unusable one from the address", () => {
    const validate = Route.options.validateSearch as (search: Record<string, unknown>) => Record<string, unknown>;
    expect(validate({ peekModule: ARCHIVED_ID, view: "list" })).toEqual({ peekModule: ARCHIVED_ID, view: "list" });
    expect(validate({ peekModule: "not-a-module" })).toEqual({});
  });

  it("mounts the shell with the active tab and per-tab title", async () => {
    renderTab(detailRow());

    await screen.findByTestId("archives-shell");
    expect(screen.getByRole("link", { name: "Modules" })).toHaveAttribute("aria-current", "page");
    await waitFor(() => expect(document.title).toBe("Web - Archived modules"));
  });

  it("mounts the archived modules screen in the shell slot", async () => {
    renderTab(detailRow(), {});

    expect(await screen.findByTestId("archived-modules-screen")).toBeInTheDocument();
    expect(screen.getByTestId("archived-modules-header")).toBeInTheDocument();
  });

  it("opens the archived peek the address names", async () => {
    renderTab(detailRow(), { peekModule: "323e4567-e89b-12d3-a456-426614174002" });

    expect(await screen.findByTestId("archived-modules-screen")).toBeInTheDocument();
    expect(screen.getByTestId("modules-peek-skeleton")).toBeInTheDocument();
  });

  it("writes the peeked module into the address from a row", async () => {
    renderTab(detailRow(), {}, [archivedRow({ id: ARCHIVED_ID })]);

    await screen.findAllByTestId("modules-row");
    fireEvent.click(screen.getByRole("button", { name: "Open preview of Archived alpha" }));

    expect(navigate).toHaveBeenCalledWith({ to: ".", search: { peekModule: ARCHIVED_ID }, replace: true });
  });

  it("lands on the work-items tab when the modules flag is off", async () => {
    const router = renderTab(detailRow({ module_view: false }));

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
