// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Transport } from "@pidash/api-client";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute as createTestRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { adminKeys } from "../queries.js";
import { resetAdminStores } from "../stores.js";
import { FIRST_WORKSPACE_CREATE_PATH, FirstWorkspacePopup } from "./FirstWorkspacePopup.js";

function hangingTransport(): Transport {
  return () => new Promise(() => {});
}

function renderPopup(workspacesExist: boolean | "pending"): { pathname: () => string; unmount: () => void } {
  expect(FIRST_WORKSPACE_CREATE_PATH).toBe("/god-mode/workspace/create/");
  createAppClient("https://api.test", hangingTransport());
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (workspacesExist !== "pending") {
    queryClient.setQueryData(adminKeys.bootstrap, { instance: { workspaces_exist: workspacesExist } } as never);
  }

  // Both pages mount the popup, as the dashboard layout does: the flag
  // outlives client-side navigation.
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const dashboardRoute = createTestRoute({
    getParentRoute: () => rootRoute,
    path: "/god-mode/general",
    component: () => (
      <>
        <div data-testid="page-general">general</div>
        <FirstWorkspacePopup />
      </>
    ),
  });
  const createRoute = createTestRoute({
    getParentRoute: () => rootRoute,
    path: "/god-mode/workspace/create",
    component: () => (
      <>
        <div data-testid="page-create">create</div>
        <FirstWorkspacePopup />
      </>
    ),
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([dashboardRoute, createRoute]),
    history: createMemoryHistory({ initialEntries: ["/god-mode/general/"] }),
    trailingSlash: "preserve",
  });
  const { unmount } = render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { pathname: () => router.state.location.pathname, unmount };
}

beforeEach(() => {
  resetAdminStores();
});

afterEach(() => {
  resetAppClient();
});

describe("FirstWorkspacePopup", () => {
  it("stays hidden while the info load is unresolved", () => {
    renderPopup("pending");
    expect(screen.queryByTestId("admin-first-workspace-popup")).not.toBeInTheDocument();
  });

  it("stays hidden when the instance holds workspaces", async () => {
    renderPopup(true);
    await waitFor(() => expect(screen.getByTestId("page-general")).toBeInTheDocument());
    expect(screen.queryByTestId("admin-first-workspace-popup")).not.toBeInTheDocument();
  });

  it("invites creating the first workspace when none exist", async () => {
    renderPopup(false);
    const popup = await screen.findByTestId("admin-first-workspace-popup");
    expect(popup).toHaveTextContent("Create your first workspace");
    expect(screen.getByRole("link", { name: "Create workspace" })).toHaveAttribute(
      "href",
      "/god-mode/workspace/create/"
    );
    expect(screen.getByRole("button", { name: "Close" })).toBeInTheDocument();
  });

  it("keeps the card across the create shortcut", async () => {
    const { pathname } = renderPopup(false);
    await screen.findByTestId("admin-first-workspace-popup");
    fireEvent.click(screen.getByRole("link", { name: "Create workspace" }));
    await waitFor(() => expect(pathname()).toBe("/god-mode/workspace/create/"));
    expect(screen.getByTestId("page-create")).toBeInTheDocument();
    expect(screen.getByTestId("admin-first-workspace-popup")).toBeInTheDocument();
  });

  it("clears on dismiss and stays clear on remount", async () => {
    const first = renderPopup(false);
    await screen.findByTestId("admin-first-workspace-popup");
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.queryByTestId("admin-first-workspace-popup")).not.toBeInTheDocument());
    first.unmount();
    renderPopup(false);
    await waitFor(() => expect(screen.getByTestId("page-general")).toBeInTheDocument());
    expect(screen.queryByTestId("admin-first-workspace-popup")).not.toBeInTheDocument();
  });
});
