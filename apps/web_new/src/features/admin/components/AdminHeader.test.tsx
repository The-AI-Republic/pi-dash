// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dashboard header: mobile hamburger plus the all-linked breadcrumb
// trail.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";

import { ADMIN_SIDEBAR_COLLAPSED_KEY } from "../adminShell.js";
import { resetAdminStores, useAdminSidebarStore } from "../stores.js";
import { AdminHeader } from "./AdminHeader.js";

function renderHeader(initialPath: string): void {
  const rootRoute = createRootRoute({ component: () => <AdminHeader /> });
  const consoleRoute = createRoute({ getParentRoute: () => rootRoute, path: "/god-mode/$" });
  const router = createRouter({
    routeTree: rootRoute.addChildren([consoleRoute]),
    history: createMemoryHistory({ initialEntries: [initialPath] }),
    trailingSlash: "preserve",
  });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  window.localStorage.clear();
  resetAdminStores();
});

describe("admin header breadcrumbs", () => {
  it("leads with settings and links every crumb to its cumulative path", async () => {
    renderHeader("/god-mode/workspace/create/");
    await screen.findByTestId("admin-header");
    const crumbs = screen.getAllByRole("listitem");
    expect(crumbs.map((crumb) => crumb.textContent)).toEqual(["Settings", "Workspace", "Create"]);
    const links = screen.getAllByRole("link");
    expect(links.map((link) => [link.textContent, link.getAttribute("href")])).toEqual([
      ["Settings", "/god-mode/general/"],
      ["Workspace", "/god-mode/workspace/"],
      ["Create", "/god-mode/workspace/create/"],
    ]);
  });

  it("maps provider segments", async () => {
    renderHeader("/god-mode/authentication/github/");
    await screen.findByTestId("admin-header");
    expect(screen.getAllByRole("link").map((link) => link.textContent)).toEqual([
      "Settings",
      "Authentication",
      "GitHub",
    ]);
  });

  it("uppercases unmapped segments and raw ids", async () => {
    renderHeader("/god-mode/loop/abc123/");
    await screen.findByTestId("admin-header");
    expect(screen.getAllByRole("link").map((link) => [link.textContent, link.getAttribute("href")])).toEqual([
      ["Settings", "/god-mode/general/"],
      ["LOOP", "/god-mode/loop/"],
      ["ABC123", "/god-mode/loop/abc123/"],
    ]);
  });
});

describe("admin hamburger", () => {
  it("hides at desktop widths and toggles the rail on small screens", async () => {
    renderHeader("/god-mode/general/");
    const hamburger = await screen.findByRole("button", { name: "Open sidebar" });
    expect(hamburger).toHaveAttribute("data-testid", "admin-hamburger");
    expect(hamburger.className).toContain("md:hidden");
    expect(useAdminSidebarStore.getState().collapsed).toBe(false);
    fireEvent.click(hamburger);
    expect(useAdminSidebarStore.getState().collapsed).toBe(true);
    expect(window.localStorage.getItem(ADMIN_SIDEBAR_COLLAPSED_KEY)).toBe("true");
  });
});
