// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// What this suite owns is the mount: the create page renders at the
// dashboard workspace create path. Page behavior belongs to the admin
// area, so the barrel is mocked out here.
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

vi.mock("../../../../features/admin/components/AdminWorkspaceCreatePage.js", () => ({
  AdminWorkspaceCreatePage: () => <div data-testid="workspace-create-page">create a workspace</div>,
}));

import { Route } from "./create.js";

describe("workspace create route", () => {
  it("mounts the create page at the dashboard workspace create path", async () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("workspace create route has no component");
    const rootRoute = createRootRoute({ component: () => <Outlet /> });
    const pageRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/god-mode/workspace/create",
      component: Component,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([pageRoute]),
      history: createMemoryHistory({ initialEntries: ["/god-mode/workspace/create/"] }),
      trailingSlash: "preserve",
    });
    render(<RouterProvider router={router} />);
    await screen.findByTestId("workspace-create-page");
    expect(screen.getByTestId("workspace-create-page")).toHaveTextContent("create a workspace");
  });
});
