// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// What this suite owns is the mount: the workspace page renders at the
// dashboard workspace path. Page behavior belongs to the admin area,
// so the barrel is mocked out here.
import { render, screen } from "@testing-library/react";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../../features/admin/components/AdminWorkspacePage.js", () => ({
  AdminWorkspacePage: () => <div data-testid="workspace-page">workspace settings</div>,
}));

import { Route } from "./index.js";

describe("workspace settings route", () => {
  it("mounts the workspace page at the dashboard workspace path", async () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("workspace route has no component");
    const rootRoute = createRootRoute({ component: () => <Outlet /> });
    const pageRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/god-mode/workspace",
      component: Component,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([pageRoute]),
      history: createMemoryHistory({ initialEntries: ["/god-mode/workspace/"] }),
      trailingSlash: "preserve",
    });
    render(<RouterProvider router={router} />);
    await screen.findByTestId("workspace-page");
    expect(screen.getByTestId("workspace-page")).toHaveTextContent("workspace settings");
  });
});
