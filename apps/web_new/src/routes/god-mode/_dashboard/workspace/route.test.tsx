// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// What this suite owns is the mount: the workspace layout renders its
// child route in the outlet (the list and create pages belong to the
// admin area).
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
import { describe, expect, it } from "vitest";

import { Route } from "./route.js";

describe("workspace layout route", () => {
  it("renders the child route in its outlet", async () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("workspace layout route has no component");
    const rootRoute = createRootRoute({ component: () => <Outlet /> });
    const layoutRoute = createRoute({
      getParentRoute: () => rootRoute,
      id: "workspace-layout",
      component: Component,
    });
    const pageRoute = createRoute({
      getParentRoute: () => layoutRoute,
      path: "/god-mode/workspace/create",
      component: () => <div data-testid="create-page">create</div>,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([layoutRoute.addChildren([pageRoute])]),
      history: createMemoryHistory({ initialEntries: ["/god-mode/workspace/create/"] }),
      trailingSlash: "preserve",
    });
    render(<RouterProvider router={router} />);
    expect(await screen.findByTestId("create-page")).toBeInTheDocument();
  });
});
