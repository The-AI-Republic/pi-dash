// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
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
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { adminKeys } from "../queries.js";
import { AdminWorkspacePage } from "./AdminWorkspacePage.js";

function hangingTransport(): Transport {
  return () => new Promise(() => {});
}

afterEach(() => {
  resetAppClient();
});

describe("AdminWorkspacePage", () => {
  it("composes the restriction toggle over the workspace list", async () => {
    createAppClient("https://api.test", hangingTransport());
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(adminKeys.configurations, [
      {
        id: "88888888-8888-4888-8888-888888888888",
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
        created_by: null,
        updated_by: null,
        deleted_at: null,
        key: "DISABLE_WORKSPACE_CREATION",
        value: "0",
        category: "test",
        is_encrypted: false,
        source: "db",
        is_managed: false,
      },
    ] as never);
    queryClient.setQueryData(adminKeys.workspaces, {
      pages: [
        {
          results: [
            {
              id: "99999999-9999-4999-8999-999999999999",
              name: "Acme",
              slug: "acme",
              logo: null,
              logo_asset: null,
              logo_url: null,
              owner: null,
              organization_size: "2-10",
              timezone: "UTC",
              background_color: "#ffffff",
              total_projects: null,
              total_members: null,
              created_by: null,
              updated_by: null,
              created_at: "2026-01-01T00:00:00Z",
              updated_at: "2026-01-01T00:00:00Z",
            },
          ],
          next_page_results: false,
          next_cursor: "",
        },
      ],
      pageParams: [undefined],
    } as never);
    const rootRoute = createRootRoute({ component: () => <Outlet /> });
    const pageRoute = createTestRoute({
      getParentRoute: () => rootRoute,
      path: "/god-mode/workspace",
      component: AdminWorkspacePage,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([pageRoute]),
      history: createMemoryHistory({ initialEntries: ["/god-mode/workspace/"] }),
      trailingSlash: "preserve",
    });
    render(
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    );
    const page = await screen.findByTestId("admin-workspace-page");
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Restrict workspace creation" })).toBeInTheDocument()
    );
    expect(page).toContainElement(screen.getByTestId("admin-workspace-toggle"));
    expect(page).toContainElement(screen.getByTestId("admin-workspace-list"));
    expect(screen.getByTestId("admin-workspace-count")).toHaveTextContent("1");
  });
});
