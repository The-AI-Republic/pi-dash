// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// What this suite owns is the mount: dashboard pages render inside
// the guard plus shell with their content in the outlet.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import type { AdminSession, Transport } from "@pidash/api-client";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { adminKeys } from "../../../features/admin/index.js";
import { resetAdminStores } from "../../../features/admin/index.js";
import { Route } from "./route.js";

const ADMIN: AdminSession = {
  kind: "admin",
  admin: { email: "admin@example.com", display_name: "Ada Admin", avatar_url: null } as never,
};

function failingTransport(): Transport {
  return (url) =>
    Promise.resolve({
      status: 500,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify({ detail: "boom" })),
    });
}

beforeEach(() => {
  window.localStorage.clear();
  resetAdminStores();
});

afterEach(() => {
  resetAppClient();
});

describe("dashboard layout route", () => {
  it("mounts the page inside the guard plus shell", async () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("dashboard layout route has no component");
    createAppClient("https://api.test", failingTransport());
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(adminKeys.session, ADMIN);
    queryClient.setQueryData(adminKeys.csrf, { csrf_token: "csrf-9" });
    queryClient.setQueryData(adminKeys.bootstrap, {
      instance: { current_version: "9.9.9", workspaces_exist: true },
    });
    const rootRoute = createRootRoute({ component: () => <Outlet /> });
    const layoutRoute = createRoute({
      getParentRoute: () => rootRoute,
      id: "dashboard-layout",
      component: Component,
    });
    const pageRoute = createRoute({
      getParentRoute: () => layoutRoute,
      path: "/god-mode/general",
      component: () => <div data-testid="settings-page">general settings</div>,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([layoutRoute.addChildren([pageRoute])]),
      history: createMemoryHistory({ initialEntries: ["/god-mode/general/"] }),
      trailingSlash: "preserve",
    });
    render(
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    );
    await screen.findByTestId("admin-dashboard");
    expect(screen.getByTestId("admin-sidebar")).toBeInTheDocument();
    expect(screen.getByTestId("admin-main")).toContainElement(screen.getByTestId("settings-page"));
  });
});
