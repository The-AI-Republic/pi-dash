// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// What this suite owns is the mount: the authentication layout renders
// its child route in the outlet (the landing and provider pages belong
// to the admin area).
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

describe("authentication layout route", () => {
  it("renders the child route in its outlet", async () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("authentication layout route has no component");
    const rootRoute = createRootRoute({ component: () => <Outlet /> });
    const layoutRoute = createRoute({
      getParentRoute: () => rootRoute,
      id: "authentication-layout",
      component: Component,
    });
    const pageRoute = createRoute({
      getParentRoute: () => layoutRoute,
      path: "/god-mode/authentication/github",
      component: () => <div data-testid="provider-page">provider</div>,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([layoutRoute.addChildren([pageRoute])]),
      history: createMemoryHistory({ initialEntries: ["/god-mode/authentication/github/"] }),
      trailingSlash: "preserve",
    });
    render(<RouterProvider router={router} />);
    expect(await screen.findByTestId("provider-page")).toBeInTheDocument();
  });
});
