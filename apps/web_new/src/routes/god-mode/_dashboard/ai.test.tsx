// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI settings route (ADM-046–048): the page mounts inside the
// dashboard layout at /god-mode/ai/. Image removal pins (ADM-049,
// ADM-050, target per main#523): the route table carries no image
// route — /god-mode/image/ falls through to the catch-all not-found
// screen with no skeleton — and the sidebar exposes no image entry.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import type { AdminSession, ApiClient, Transport } from "@pidash/api-client";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ADMIN_SIDEBAR_ENTRIES, adminKeys, AdminSidebar, resetAdminStores } from "../../../features/admin/index.js";
import { routeTree } from "../../../routeTree.gen.js";

const ADMIN: AdminSession = {
  kind: "admin",
  admin: { email: "admin@example.com", display_name: "Ada Admin", avatar_url: null } as never,
};

function configurationRow(key: string, value: string | null): Record<string, unknown> {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    key,
    value,
    category: "ai",
    is_encrypted: false,
    source: "db",
    is_managed: true,
  };
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

function seedConsole(queryClient: QueryClient): void {
  queryClient.setQueryData(adminKeys.session, ADMIN);
  queryClient.setQueryData(adminKeys.csrf, { csrf_token: "csrf-9" });
  queryClient.setQueryData(adminKeys.bootstrap, {
    instance: { current_version: "9.9.9", workspaces_exist: true },
  });
  queryClient.setQueryData(adminKeys.configurations, [
    configurationRow("LLM_MODEL", "gpt-4o"),
    configurationRow("LLM_API_KEY", "sk-live"),
  ]);
}

function renderAt(initialPath: string): void {
  const apiClient: ApiClient = createAppClient("https://api.test", failingTransport());
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  seedConsole(queryClient);
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: [initialPath] }),
    context: { queryClient, apiClient },
    trailingSlash: "preserve",
  });
  render(<RouterProvider router={router} />);
}

beforeEach(() => {
  window.localStorage.clear();
  resetAdminStores();
});

afterEach(() => {
  resetAppClient();
});

describe("AI settings route", () => {
  it("mounts the AI page inside the dashboard shell", async () => {
    renderAt("/god-mode/ai/");
    await screen.findByTestId("admin-dashboard");
    expect(screen.getByTestId("admin-sidebar")).toBeInTheDocument();
    const page = await screen.findByTestId("admin-ai-page");
    expect(screen.getByTestId("admin-main")).toContainElement(page);
    expect(screen.getByTestId("admin-ai-form")).toBeInTheDocument();
  });
});

describe("image removal pins", () => {
  it("falls through /god-mode/image/ to the not-found screen with no skeleton", async () => {
    renderAt("/god-mode/image/");
    await screen.findByTestId("admin-not-found");
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.queryByTestId("admin-ai-page")).not.toBeInTheDocument();
    expect(screen.queryByTestId("admin-dashboard")).not.toBeInTheDocument();
  });

  it("exposes no image entry in the sidebar", async () => {
    expect(ADMIN_SIDEBAR_ENTRIES.some((entry) => entry.slug === "image")).toBe(false);
    createAppClient("https://api.test", failingTransport());
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    seedConsole(queryClient);
    const rootRoute = createRootRoute({ component: () => <AdminSidebar /> });
    const consoleRoute = createRoute({ getParentRoute: () => rootRoute, path: "/god-mode/$" });
    const router = createRouter({
      routeTree: rootRoute.addChildren([consoleRoute]),
      history: createMemoryHistory({ initialEntries: ["/god-mode/ai/"] }),
      trailingSlash: "preserve",
    });
    render(
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    );
    await screen.findByTestId("admin-sidebar");
    expect(screen.queryByTestId("admin-nav-image")).not.toBeInTheDocument();
    for (const link of screen.getAllByRole("link")) {
      expect(link.getAttribute("href")).not.toContain("image");
    }
    expect(screen.getByTestId("admin-nav-ai")).toHaveAttribute("href", "/god-mode/ai/");
  });
});
