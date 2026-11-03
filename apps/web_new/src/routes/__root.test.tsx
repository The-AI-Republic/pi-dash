// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { createClient } from "@pidash/api-client";
import { createMemoryHistory, createRoute, createRouter, RouterProvider } from "@tanstack/react-router";
import * as React from "react";
import { afterEach, expect, it, vi } from "vitest";

const lifecycle = vi.hoisted(() => vi.fn(() => <p>runtime sign-out lifecycle</p>));
const locationHarness = vi.hoisted(() => ({ pathname: "/" }));

vi.mock("@tanstack/react-router", async () => {
  const actual = await vi.importActual<typeof import("@tanstack/react-router")>("@tanstack/react-router");
  return { ...actual, useLocation: () => ({ pathname: locationHarness.pathname }) };
});

vi.mock("../core/i18n/LanguageProvider.js", () => ({
  LanguageProvider: ({ children }: { children: React.ReactNode }) => children,
}));

vi.mock("../core/theme/ThemeProvider.js", () => ({
  ThemeProvider: ({ children }: { children: React.ReactNode }) => children,
}));

vi.mock("../features/runners/runtime/SignOutLifecycle.js", () => ({
  RuntimeSignOutLifecycle: lifecycle,
}));

vi.mock("../shared/shell/index.js", () => ({
  NotFoundPage: () => null,
  markInitialColorScheme: vi.fn(),
  notifyError: vi.fn(),
  useDocumentHead: vi.fn(),
}));

import { edition } from "../core/edition/index.js";
import { Route } from "./__root.js";

afterEach(() => {
  delete edition.routes;
  locationHarness.pathname = "/";
});

it("mounts the runtime sign-out lifecycle above every route", async () => {
  vi.stubGlobal("scrollTo", vi.fn());
  const childRoute = createRoute({
    getParentRoute: () => Route,
    path: "/",
    component: () => <p>route content</p>,
  });
  const router = createRouter({
    routeTree: Route.addChildren([childRoute]),
    context: {
      queryClient: new QueryClient(),
      apiClient: createClient({
        baseUrl: "https://api.test",
        transport: () => {
          throw new Error("unexpected request");
        },
      }),
    },
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });

  render(<RouterProvider router={router} />);
  expect(await screen.findByText("route content")).toBeVisible();
  expect(screen.getByText("runtime sign-out lifecycle")).toBeVisible();
  expect(lifecycle).toHaveBeenCalled();
});

async function renderRouteWithPath(path: string): Promise<void> {
  vi.stubGlobal("scrollTo", vi.fn());
  locationHarness.pathname = path;
  vi.clearAllMocks();
  const childRoute = createRoute({
    getParentRoute: () => Route,
    path: "$",
    component: () => <p>route content</p>,
  });
  const router = createRouter({
    routeTree: Route.addChildren([childRoute]),
    context: {
      queryClient: new QueryClient(),
      apiClient: createClient({
        baseUrl: "https://api.test",
        transport: () => {
          throw new Error("unexpected request");
        },
      }),
    },
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  render(<RouterProvider router={router} />);
  expect(await screen.findByText("route content")).toBeVisible();
}

it("mounts a declared marketing page without the authenticated provider stack (CLOUD-005)", async () => {
  edition.routes = [{ path: "home", scope: "top-level" }];
  await renderRouteWithPath("/home");
  expect(lifecycle).not.toHaveBeenCalled();
});

it("keeps the provider stack on an undeclared marketing path", async () => {
  await renderRouteWithPath("/docs");
  expect(lifecycle).toHaveBeenCalled();
});

it("keeps the provider stack at the edition's root and on app routes", async () => {
  edition.routes = [{ path: "/", scope: "top-level" }];
  await renderRouteWithPath("/");
  expect(lifecycle).toHaveBeenCalled();
});
