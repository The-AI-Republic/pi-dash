// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
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
import { resetAdminStores } from "../stores.js";
import {
  AdminWorkspaceCreatePage,
  WORKSPACE_CREATE_BACK_PATH,
  WORKSPACE_CREATE_HEADING,
  WORKSPACE_CREATE_PAGE_TITLE,
} from "./AdminWorkspaceCreatePage.js";

function hangingTransport(): Transport {
  return () => new Promise(() => {});
}

async function renderPage(): Promise<void> {
  createAppClient("https://api.test", hangingTransport());
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const createRoute = createTestRoute({
    getParentRoute: () => rootRoute,
    path: "/god-mode/workspace/create",
    component: () => <AdminWorkspaceCreatePage />,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([createRoute]),
    history: createMemoryHistory({ initialEntries: ["/god-mode/workspace/create/"] }),
    trailingSlash: "preserve",
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  await screen.findByTestId("admin-workspace-create-page");
}

beforeEach(() => {
  resetAdminStores();
});

afterEach(() => {
  resetAppClient();
});

describe("AdminWorkspaceCreatePage", () => {
  it("titles the document and heads the page", async () => {
    expect(WORKSPACE_CREATE_PAGE_TITLE).toBe("Create workspace — Pi Dash");
    await renderPage();
    expect(document.title).toBe(WORKSPACE_CREATE_PAGE_TITLE);
    expect(screen.getByTestId("admin-workspace-create-heading")).toHaveTextContent(WORKSPACE_CREATE_HEADING);
    expect(screen.getByTestId("admin-workspace-create-helper")).not.toBeEmptyDOMElement();
  });

  it("links back to the workspace list and mounts the form", async () => {
    expect(WORKSPACE_CREATE_BACK_PATH).toBe("/god-mode/workspace");
    await renderPage();
    const back = screen.getByTestId("admin-workspace-create-back") as HTMLAnchorElement;
    expect(back.getAttribute("href")).toContain("/workspace");
    expect(back).toHaveTextContent("Back to workspaces");
    expect(screen.getByTestId("admin-workspace-create-form")).toBeInTheDocument();
    expect(screen.getByTestId("admin-workspace-create-submit")).toBeInTheDocument();
  });
});
