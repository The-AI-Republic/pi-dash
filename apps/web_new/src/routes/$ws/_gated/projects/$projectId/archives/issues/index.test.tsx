// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
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
import { Route } from "./index.js";

// The tab body owns the list and its reads; this suite proves the route
// hands it the address selection and writes/clears the peek params
// without pushing history.
const archiveScreenProps = vi.hoisted(() => ({ current: null as Record<string, unknown> | null }));
const archiveNavigate = vi.hoisted(() => ({ fn: vi.fn() }));
vi.mock("../../../../../../../features/issues/contexts/ArchivedIssuesScreen.js", () => ({
  ArchivedIssuesScreen: (props: Record<string, unknown>) => {
    archiveScreenProps.current = props;
    return <div data-testid="archived-screen" />;
  },
}));
vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    useNavigate:
      () =>
      (...args: unknown[]) =>
        archiveNavigate.fn(...args),
  };
});

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

function renderTab(seedRow: boolean) {
  vi.stubGlobal("matchMedia", () => ({
    matches: false,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: PROJECT_ID });
  const Component = Route.options.component;
  if (!Component) throw new Error("archived issues tab has no component");
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (seedRow) queryClient.setQueryData(authKeys.project("acme", PROJECT_ID), detailRow());
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

describe("archived work-items tab route", () => {
  it("mounts the shell with the active tab and per-tab title", async () => {
    renderTab(true);

    await screen.findByTestId("archives-shell");
    expect(screen.getByRole("link", { name: "Work items" })).toHaveAttribute("aria-current", "page");
    await waitFor(() => expect(document.title).toBe("Web - Archived work items"));
  });

  it("holds the fallback title while the project row resolves", async () => {
    const router = renderTab(false);

    await waitFor(() => expect(router.state.isLoading).toBe(false));
    expect(document.title).toBe("Archives");
    expect(screen.queryByTestId("archives-shell")).toBeNull();
  });
});

describe("archived work-items tab peek address", () => {
  it("clears every peek param when the selection clears", async () => {
    vi.spyOn(Route, "useSearch").mockReturnValue({
      peekIssueId: UUID,
      peekProjectId: "p-2",
      peekNestingLevel: "1",
      layout: "compact",
    } as never);
    renderTab(true);
    await screen.findByTestId("archived-screen");
    const onPeekChange = archiveScreenProps.current?.["onPeekChange"] as (id: string | null) => void;
    onPeekChange(null);
    expect(archiveNavigate.fn).toHaveBeenCalledWith({
      to: ".",
      search: { layout: "compact" },
      replace: true,
    });
  });

  it("writes the item and drops the nested peek params", async () => {
    vi.spyOn(Route, "useSearch").mockReturnValue({
      peekIssueId: UUID,
      peekProjectId: "p-2",
      peekNestingLevel: "1",
      layout: "compact",
    } as never);
    renderTab(true);
    await screen.findByTestId("archived-screen");
    const onPeekChange = archiveScreenProps.current?.["onPeekChange"] as (id: string | null) => void;
    onPeekChange("423e4567-e89b-12d3-a456-426614174003");
    expect(archiveNavigate.fn).toHaveBeenCalledWith({
      to: ".",
      search: { peekIssueId: "423e4567-e89b-12d3-a456-426614174003", layout: "compact" },
      replace: true,
    });
  });
});
