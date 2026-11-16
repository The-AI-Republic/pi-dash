// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace home wiring (NEWFRONT-278, NEWFRONT-280, NEWFRONT-279). The
// route keeps the tour seam mounted, hands its path params plus the
// projects read to the dashboard with the widget stack in its `widgets`
// slot, and warms the assistant card reads in its loader; the widget-map
// read stays off the loader so first paint shows the dashboard skeleton.
// The dashboard's sections belong to the home feature, so this suite only
// proves the composition and the loader's cache writes.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
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

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import type { RouterContext } from "../../__root.js";

vi.mock("../../../features/auth/components/WorkspaceHomeTour.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../features/auth/components/WorkspaceHomeTour.js")>();
  return { ...actual, WorkspaceHomeTour: () => <div data-testid="tour-seam" /> };
});

vi.mock("../../../features/home/components/HomeDashboard.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../features/home/components/HomeDashboard.js")>();
  return {
    ...actual,
    HomeDashboard: (props: {
      workspaceSlug: string;
      projects: unknown;
      widgets?: React.ReactNode;
      onCreateProject?: () => void;
    }) => (
      <div
        data-testid="home-dashboard"
        data-slug={props.workspaceSlug}
        data-projects={props.projects === undefined ? "loading" : "ready"}
      >
        {props.widgets}
        {props.onCreateProject === undefined ? null : (
          <button type="button" data-testid="quickstart-create" onClick={props.onCreateProject}>
            quickstart create
          </button>
        )}
      </div>
    ),
  };
});
vi.mock("../../../features/home/components/HomeWidgetStack.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../features/home/components/HomeWidgetStack.js")>();
  return {
    ...actual,
    HomeWidgetStack: (props: {
      workspaceSlug: string;
      projects: unknown;
      quicklinks?: React.ReactNode;
      manageWidgets?: (controls: { open: boolean; onOpenChange: (open: boolean) => void }) => React.ReactNode;
      emptyState?: React.ReactNode;
    }) => (
      <div
        data-testid="widget-stack"
        data-slug={props.workspaceSlug}
        data-projects={props.projects === undefined ? "loading" : "ready"}
      >
        {props.quicklinks}
        {props.manageWidgets?.({
          open: false,
          onOpenChange: () => {},
        })}
        {props.emptyState}
      </div>
    ),
  };
});
vi.mock("../../../features/home/components/QuicklinksWidget.js", () => ({
  QuicklinksWidget: (props: { workspaceSlug: string }) => (
    <div data-testid="quicklinks-slot" data-slug={props.workspaceSlug} />
  ),
}));
vi.mock("../../../features/home/components/ManageWidgetsDialog.js", () => ({
  ManageWidgetsDialog: (props: { workspaceSlug: string; open: boolean }) => (
    <div data-testid="manage-slot" data-slug={props.workspaceSlug} data-open={String(props.open)} />
  ),
}));
vi.mock("../../../features/home/components/HomeAllOffState.js", () => ({
  HomeAllOffState: () => <div data-testid="all-off-slot" />,
}));

import { assistantKeys } from "../../../features/assistant/index.js";
import { widgetPrefsKeys } from "../../../features/home/api/widgetPrefs.js";
import { projectKeys } from "../../../features/projects/api/keys.js";
import { Route } from "./index.js";

const THREAD = {
  id: "11111111-1111-4111-8111-111111111111",
  title: "Sprint summary",
  is_archived: false,
  has_active_turn: false,
  created_at: "2026-10-01T00:00:00Z",
  updated_at: "2026-10-02T00:00:00Z",
};

const CONFIG = {
  provider_kind: "openai",
  base_url: "https://api.test",
  model_name: "pilot",
  has_api_key: true,
  last_verified_at: null,
};

function stubApiClient(body: (url: string) => unknown) {
  return {
    getJson: async (url: string) => body(url),
    parse: (_schema: unknown, data: unknown) => data,
  } as never;
}

function renderHome(): void {
  const apiClient = createAppClient("https://api.test", (async (url: string) => ({
    status: 500,
    url,
    headers: { get: () => null },
    text: async () => "{}",
  })) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(projectKeys.list("acme"), []);
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
  const indexRoute = createRoute({
    getParentRoute: () => gatedRoute,
    path: "/",
    component,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([wsRoute.addChildren([gatedRoute.addChildren([indexRoute])])]),
    context: { queryClient, apiClient },
    history: createMemoryHistory({ initialEntries: ["/acme/"] }),
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

describe("workspace home route", () => {
  it("mounts the tour seam, the dashboard, and the widget stack with their params", async () => {
    renderHome();
    expect(await screen.findByTestId("tour-seam")).toBeInTheDocument();
    const dashboard = screen.getByTestId("home-dashboard");
    expect(dashboard).toHaveAttribute("data-slug", "acme");
    expect(dashboard).toHaveAttribute("data-projects", "ready");
    const stack = screen.getByTestId("widget-stack");
    expect(stack).toHaveAttribute("data-slug", "acme");
    expect(stack).toHaveAttribute("data-projects", "ready");
  });

  it("hosts the shared create dialog for the quickstart card", async () => {
    renderHome();
    expect(await screen.findByTestId("quickstart-create")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "New project" })).toBeNull();

    fireEvent.click(screen.getByTestId("quickstart-create"));
    expect(screen.getByRole("heading", { name: "New project" })).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("heading", { name: "New project" })).toBeNull();
  });

  it("fills the stack's quicklinks, manage-widgets, and empty slots", async () => {
    renderHome();
    expect(await screen.findByTestId("widget-stack")).toBeInTheDocument();
    expect(screen.getByTestId("quicklinks-slot")).toHaveAttribute("data-slug", "acme");
    expect(screen.getByTestId("manage-slot")).toHaveAttribute("data-slug", "acme");
    expect(screen.getByTestId("manage-slot")).toHaveAttribute("data-open", "false");
    expect(screen.getByTestId("all-off-slot")).toBeInTheDocument();
  });

  it("warms the assistant card reads in its loader", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const apiClient = stubApiClient((url: string) => {
      if (url.endsWith("/ai-assistant/threads/")) return [THREAD];
      if (url.endsWith("/ai-assistant/config/")) return CONFIG;
      throw new Error(`unexpected ${url}`);
    });
    const loader = Route.options.loader;
    if (typeof loader !== "function") throw new Error("missing loader");
    await loader({ context: { queryClient, apiClient }, params: { ws: "acme" } } as never);
    expect(queryClient.getQueryData(assistantKeys.threads("acme"))).toEqual([THREAD]);
    expect(queryClient.getQueryData(assistantKeys.config("acme"))).toEqual(CONFIG);
  });

  it("tolerates failed assistant reads in its loader", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const apiClient = stubApiClient(() => {
      throw new Error("denied");
    });
    const loader = Route.options.loader;
    if (typeof loader !== "function") throw new Error("missing loader");
    await expect(loader({ context: { queryClient, apiClient }, params: { ws: "acme" } } as never)).resolves.toEqual({});
    expect(queryClient.getQueryData(assistantKeys.threads("acme"))).toBeUndefined();
    expect(queryClient.getQueryData(assistantKeys.config("acme"))).toBeUndefined();
  });

  it("leaves the widget-map read off the loader so first paint shows the skeleton", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const seen: string[] = [];
    const apiClient = stubApiClient((url: string) => {
      seen.push(url);
      if (url.endsWith("/ai-assistant/threads/")) return [THREAD];
      if (url.endsWith("/ai-assistant/config/")) return CONFIG;
      throw new Error(`unexpected ${url}`);
    });
    const loader = Route.options.loader;
    if (typeof loader !== "function") throw new Error("missing loader");
    await loader({ context: { queryClient, apiClient }, params: { ws: "acme" } } as never);
    expect(queryClient.getQueryData(widgetPrefsKeys.list("acme"))).toBeUndefined();
    expect(seen.some((url) => url.includes("/home-preferences/"))).toBe(false);
  });
});
