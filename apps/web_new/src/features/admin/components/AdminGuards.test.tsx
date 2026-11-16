// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
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
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { adminKeys } from "../queries.js";
import { AdminDashboardGuard, AdminHomeGuard } from "./AdminGuards.js";

const ADMIN: AdminSession = { kind: "admin", admin: { email: "admin@example.com" } as never };
const ANONYMOUS: AdminSession = { kind: "anonymous" };
const FORBIDDEN: AdminSession = { kind: "forbidden" };

/** A transport that never settles, holding the session check unresolved. */
function hangingTransport(): Transport {
  return () => new Promise(() => {});
}

function failingTransport(): Transport {
  return (url) =>
    Promise.resolve({
      status: 500,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify({ detail: "boom" })),
    });
}

function renderTree(
  initialPath: string,
  session: AdminSession | "pending" | "failed"
): { historyLength: () => number; pathname: () => string } {
  createAppClient("https://api.test", session === "pending" ? hangingTransport() : failingTransport());
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (session !== "pending" && session !== "failed") {
    queryClient.setQueryData(adminKeys.session, session);
  }

  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const homeRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/god-mode",
    component: () => (
      <AdminHomeGuard>
        <div data-testid="home-content">entry</div>
      </AdminHomeGuard>
    ),
  });
  const dashboardRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/god-mode/general",
    component: () => (
      <AdminDashboardGuard>
        <div data-testid="dash-content">dashboard</div>
      </AdminDashboardGuard>
    ),
  });
  // Trailing slashes preserved, mirroring the app router: the console
  // uses the canonical trailing-slash form.
  const router = createRouter({
    routeTree: rootRoute.addChildren([homeRoute, dashboardRoute]),
    history: createMemoryHistory({ initialEntries: [initialPath] }),
    trailingSlash: "preserve",
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { historyLength: () => router.history.length, pathname: () => router.state.location.pathname };
}

afterEach(() => {
  resetAppClient();
});

describe("AdminHomeGuard", () => {
  it("shows only the loader while the check is unresolved", async () => {
    renderTree("/god-mode/", "pending");
    await waitFor(() => expect(screen.getByTestId("admin-loader")).toBeInTheDocument());
    expect(screen.queryByTestId("home-content")).not.toBeInTheDocument();
  });

  it("replace-navigates a signed-in admin to general settings", async () => {
    const { historyLength, pathname } = renderTree("/god-mode/", ADMIN);
    await waitFor(() => expect(screen.getByTestId("dash-content")).toBeInTheDocument());
    expect(screen.queryByTestId("home-content")).not.toBeInTheDocument();
    expect(pathname()).toBe("/god-mode/general/");
    expect(historyLength()).toBe(1);
  });

  it("renders the entry for an anonymous check", async () => {
    renderTree("/god-mode/", ANONYMOUS);
    await waitFor(() => expect(screen.getByTestId("home-content")).toBeInTheDocument());
    expect(screen.queryByTestId("admin-loader")).not.toBeInTheDocument();
    expect(screen.queryByTestId("dash-content")).not.toBeInTheDocument();
  });

  it("renders the entry when the grant was revoked", async () => {
    renderTree("/god-mode/", FORBIDDEN);
    await waitFor(() => expect(screen.getByTestId("home-content")).toBeInTheDocument());
    expect(screen.queryByTestId("dash-content")).not.toBeInTheDocument();
  });

  it("renders the entry when the check itself fails", async () => {
    renderTree("/god-mode/", "failed");
    await waitFor(() => expect(screen.getByTestId("home-content")).toBeInTheDocument());
    expect(screen.queryByTestId("dash-content")).not.toBeInTheDocument();
  });
});

describe("AdminDashboardGuard", () => {
  it("shows only the loader while the check is unresolved", async () => {
    renderTree("/god-mode/general/", "pending");
    await waitFor(() => expect(screen.getByTestId("admin-loader")).toBeInTheDocument());
    expect(screen.queryByTestId("dash-content")).not.toBeInTheDocument();
  });

  it("replace-navigates anonymous visitors to the root", async () => {
    const { historyLength, pathname } = renderTree("/god-mode/general/", ANONYMOUS);
    await waitFor(() => expect(screen.getByTestId("home-content")).toBeInTheDocument());
    expect(screen.queryByTestId("dash-content")).not.toBeInTheDocument();
    expect(pathname()).toBe("/god-mode/");
    expect(historyLength()).toBe(1);
  });

  it("bounces a revoked grant to the root", async () => {
    const { pathname } = renderTree("/god-mode/general/", FORBIDDEN);
    await waitFor(() => expect(screen.getByTestId("home-content")).toBeInTheDocument());
    expect(pathname()).toBe("/god-mode/");
  });

  it("bounces a failed check to the root", async () => {
    const { pathname } = renderTree("/god-mode/general/", "failed");
    await waitFor(() => expect(screen.getByTestId("home-content")).toBeInTheDocument());
    expect(pathname()).toBe("/god-mode/");
  });

  it("renders the dashboard for a signed-in admin", async () => {
    renderTree("/god-mode/general/", ADMIN);
    await waitFor(() => expect(screen.getByTestId("dash-content")).toBeInTheDocument());
    expect(screen.queryByTestId("admin-loader")).not.toBeInTheDocument();
  });
});
