// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dashboard layout: guard wrap, stored-theme apply on mount, sidebar
// plus header chrome, page outlet, and the first-workspace popup.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
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
import { notifySuccess } from "../../../shared/shell/index.js";
import { adminKeys } from "../queries.js";
import { resetAdminStores } from "../stores.js";
import { ADMIN_THEME_KEY } from "../theme.js";
import { AdminDashboardLayout } from "./AdminDashboardLayout.js";

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

function hangingTransport(): Transport {
  return () => new Promise(() => {});
}

function renderLayout(session: AdminSession | "pending", workspacesExist: boolean): { pathname: () => string } {
  createAppClient("https://api.test", session === "pending" ? hangingTransport() : failingTransport());
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (session !== "pending") {
    queryClient.setQueryData(adminKeys.session, session);
  }
  queryClient.setQueryData(adminKeys.csrf, { csrf_token: "csrf-9" });
  queryClient.setQueryData(adminKeys.bootstrap, {
    instance: { current_version: "9.9.9", workspaces_exist: workspacesExist },
  });
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const homeRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/god-mode",
    component: () => <div data-testid="entry">entry</div>,
  });
  const dashboardRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/god-mode/general",
    component: () => (
      <AdminDashboardLayout>
        <div data-testid="page">general settings</div>
      </AdminDashboardLayout>
    ),
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([homeRoute, dashboardRoute]),
    history: createMemoryHistory({ initialEntries: ["/god-mode/general/"] }),
    trailingSlash: "preserve",
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { pathname: () => router.state.location.pathname };
}

beforeEach(() => {
  window.localStorage.clear();
  resetAdminStores();
});

afterEach(() => {
  resetAppClient();
});

describe("AdminDashboardLayout", () => {
  it("mounts the shell around the page for a signed-in admin", async () => {
    renderLayout(ADMIN, true);
    await waitFor(() => expect(screen.getByTestId("admin-dashboard")).toBeInTheDocument());
    expect(screen.getByTestId("admin-sidebar")).toBeInTheDocument();
    expect(screen.getByTestId("admin-header")).toBeInTheDocument();
    expect(screen.getByTestId("admin-main")).toContainElement(screen.getByTestId("page"));
    expect(screen.queryByTestId("admin-first-workspace-popup")).not.toBeInTheDocument();
  });

  it("hosts the toast stack for page notices", async () => {
    renderLayout(ADMIN, true);
    await waitFor(() => expect(screen.getByTestId("toast-stack")).toBeInTheDocument());
    expect(screen.getByTestId("admin-dashboard")).toContainElement(screen.getByTestId("toast-stack"));
  });

  it("applies the stored theme on mount", async () => {
    window.localStorage.setItem(ADMIN_THEME_KEY, "dark");
    renderLayout(ADMIN, true);
    await waitFor(() => expect(screen.getByTestId("admin-dashboard")).toBeInTheDocument());
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("raises the first-workspace popup above the page when the instance has none", async () => {
    renderLayout(ADMIN, false);
    await waitFor(() => expect(screen.getByTestId("admin-first-workspace-popup")).toBeInTheDocument());
    expect(screen.getByTestId("page")).toHaveTextContent("general settings");
  });

  it("shows only the loader while the session check is unresolved", async () => {
    renderLayout("pending", true);
    await waitFor(() => expect(screen.getByTestId("admin-loader")).toBeInTheDocument());
    expect(screen.queryByTestId("admin-dashboard")).not.toBeInTheDocument();
    expect(screen.queryByTestId("page")).not.toBeInTheDocument();
  });

  it("replace-navigates anyone else to the root", async () => {
    const { pathname } = renderLayout({ kind: "anonymous" }, true);
    await waitFor(() => expect(screen.getByTestId("entry")).toBeInTheDocument());
    expect(screen.queryByTestId("admin-dashboard")).not.toBeInTheDocument();
    expect(pathname()).toBe("/god-mode/");
  });

  it("renders page notices through the mounted toast host", async () => {
    renderLayout(ADMIN, true);
    await waitFor(() => expect(screen.getByTestId("admin-dashboard")).toBeInTheDocument());
    notifySuccess("Success", "Configuration saved successfully");
    await waitFor(() => expect(screen.getByTestId("toast-notification")).toBeInTheDocument());
    expect(screen.getByTestId("toast-notification")).toHaveTextContent("Configuration saved successfully");
  });
});
