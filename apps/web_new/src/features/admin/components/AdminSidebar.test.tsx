// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dashboard sidebar: entry navigation with prefix highlight, collapse
// persistence, narrow-viewport one-way auto-collapse, small-screen
// outside-tap collapse, and the off-canvas mobile rail.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen } from "@testing-library/react";
import type { AdminSession, Transport } from "@pidash/api-client";
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
import { ADMIN_SIDEBAR_COLLAPSED_KEY } from "../adminShell.js";
import { adminKeys } from "../queries.js";
import { resetAdminStores, useAdminSidebarStore } from "../stores.js";
import { AdminSidebar } from "./AdminSidebar.js";

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

async function renderSidebar(initialPath: string): Promise<void> {
  createAppClient("https://api.test", failingTransport());
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(adminKeys.session, ADMIN);
  queryClient.setQueryData(adminKeys.csrf, { csrf_token: "csrf-9" });
  queryClient.setQueryData(adminKeys.bootstrap, {
    instance: { current_version: "9.9.9", workspaces_exist: true },
  });
  const rootRoute = createRootRoute({ component: () => <AdminSidebar /> });
  const consoleRoute = createRoute({ getParentRoute: () => rootRoute, path: "/god-mode/$" });
  const router = createRouter({
    routeTree: rootRoute.addChildren([consoleRoute]),
    history: createMemoryHistory({ initialEntries: [initialPath] }),
    trailingSlash: "preserve",
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  await screen.findByTestId("admin-sidebar");
}

function setViewportWidth(width: number): void {
  Object.defineProperty(window, "innerWidth", { value: width, configurable: true });
}

beforeEach(() => {
  setViewportWidth(1440);
  window.localStorage.clear();
  resetAdminStores();
});

afterEach(() => {
  setViewportWidth(1440);
  resetAppClient();
});

describe("sidebar entries", () => {
  it("lists every settings area with name, description, and route", async () => {
    await renderSidebar("/god-mode/general/");
    const links = screen.getAllByTestId(/^admin-nav-/);
    expect(links.map((link) => link.textContent)).toEqual([
      expect.stringContaining("General"),
      expect.stringContaining("Email"),
      expect.stringContaining("Authentication"),
      expect.stringContaining("Workspaces"),
      expect.stringContaining("Artificial intelligence"),
      expect.stringContaining("Loop"),
      expect.stringContaining("Runners"),
    ]);
    expect(links).toHaveLength(7);
    expect(links.map((link) => link.getAttribute("href"))).toEqual([
      "/god-mode/general/",
      "/god-mode/email/",
      "/god-mode/authentication/",
      "/god-mode/workspace/",
      "/god-mode/ai/",
      "/god-mode/loop/",
      "/god-mode/runners/",
    ]);
    for (const link of links) {
      expect(link.querySelector("svg")).toBeInTheDocument();
    }
  });

  it("highlights the entry matching the current path", async () => {
    await renderSidebar("/god-mode/email/");
    expect(screen.getByTestId("admin-nav-email")).toHaveAttribute("aria-current", "page");
    expect(screen.getByTestId("admin-nav-general")).not.toHaveAttribute("aria-current");
  });

  it("keeps the parent highlighted on sub-pages", async () => {
    await renderSidebar("/god-mode/authentication/github/");
    expect(screen.getByTestId("admin-nav-authentication")).toHaveAttribute("aria-current", "page");
    for (const slug of ["general", "email", "workspace", "ai", "loop", "runners"]) {
      expect(screen.getByTestId(`admin-nav-${slug}`)).not.toHaveAttribute("aria-current");
    }
  });

  it("mounts the account menu and help footer around the entries", async () => {
    await renderSidebar("/god-mode/general/");
    expect(screen.getByTestId("admin-account-label")).toHaveTextContent("Instance admin");
    expect(screen.getByTestId("admin-help-redirect")).toHaveAttribute("href", "/");
  });
});

describe("collapsed rail", () => {
  it("narrows to icon routes with the labels hidden", async () => {
    useAdminSidebarStore.getState().setCollapsed(true);
    await renderSidebar("/god-mode/general/");
    const links = screen.getAllByTestId(/^admin-nav-/);
    expect(links).toHaveLength(7);
    expect(links.map((link) => link.getAttribute("href"))).toEqual([
      "/god-mode/general/",
      "/god-mode/email/",
      "/god-mode/authentication/",
      "/god-mode/workspace/",
      "/god-mode/ai/",
      "/god-mode/loop/",
      "/god-mode/runners/",
    ]);
    for (const link of links) {
      expect(link.textContent).toBe("");
      expect(link.querySelector("svg")).toBeInTheDocument();
    }
    const rail = screen.getByTestId("admin-sidebar");
    expect(rail.className).toContain("-translate-x-full");
    expect(rail.className).toContain("md:w-16");
    expect(rail.className).toContain("md:translate-x-0");
  });

  it("renders the full rail in flow when expanded", async () => {
    await renderSidebar("/god-mode/general/");
    const rail = screen.getByTestId("admin-sidebar");
    expect(rail.className).toContain("w-72");
    expect(rail.className).not.toContain("-translate-x-full");
    expect(rail.className).toContain("md:relative");
  });
});

describe("responsive collapse", () => {
  it("mounts collapsed on narrow viewports and persists the choice", async () => {
    setViewportWidth(700);
    await renderSidebar("/god-mode/general/");
    expect(useAdminSidebarStore.getState().collapsed).toBe(true);
    expect(window.localStorage.getItem(ADMIN_SIDEBAR_COLLAPSED_KEY)).toBe("true");
  });

  it("mounts expanded on wide viewports", async () => {
    await renderSidebar("/god-mode/general/");
    expect(useAdminSidebarStore.getState().collapsed).toBe(false);
    expect(window.localStorage.getItem(ADMIN_SIDEBAR_COLLAPSED_KEY)).toBeNull();
  });

  it("auto-collapses one-way when the viewport narrows", async () => {
    await renderSidebar("/god-mode/general/");
    setViewportWidth(700);
    fireEvent(window, new Event("resize"));
    expect(useAdminSidebarStore.getState().collapsed).toBe(true);
    expect(window.localStorage.getItem(ADMIN_SIDEBAR_COLLAPSED_KEY)).toBe("true");
    // Widening back never re-expands.
    setViewportWidth(1440);
    fireEvent(window, new Event("resize"));
    expect(useAdminSidebarStore.getState().collapsed).toBe(true);
  });

  it("collapses on an outside tap on small screens", async () => {
    setViewportWidth(700);
    // Start open: the mount rule would collapse, so expand after mount.
    await renderSidebar("/god-mode/general/");
    act(() => useAdminSidebarStore.getState().setCollapsed(false));
    fireEvent.pointerDown(document.body);
    expect(useAdminSidebarStore.getState().collapsed).toBe(true);
  });

  it("ignores taps inside the rail on small screens", async () => {
    setViewportWidth(700);
    await renderSidebar("/god-mode/general/");
    act(() => useAdminSidebarStore.getState().setCollapsed(false));
    fireEvent.pointerDown(screen.getByTestId("admin-nav-general"));
    expect(useAdminSidebarStore.getState().collapsed).toBe(false);
  });

  it("ignores outside taps at desktop widths", async () => {
    await renderSidebar("/god-mode/general/");
    fireEvent.pointerDown(document.body);
    expect(useAdminSidebarStore.getState().collapsed).toBe(false);
  });

  it("ignores outside taps while already collapsed", async () => {
    setViewportWidth(700);
    await renderSidebar("/god-mode/general/");
    expect(useAdminSidebarStore.getState().collapsed).toBe(true);
    fireEvent.pointerDown(document.body);
    expect(useAdminSidebarStore.getState().collapsed).toBe(true);
  });
});
