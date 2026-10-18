// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { createClient } from "@pidash/api-client";
import { createMemoryHistory, createRoute, createRouter, RouterProvider } from "@tanstack/react-router";
import * as React from "react";
import { expect, it, vi } from "vitest";

const lifecycle = vi.hoisted(() => vi.fn(() => <p>runtime sign-out lifecycle</p>));

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

import { Route } from "./__root.js";

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
