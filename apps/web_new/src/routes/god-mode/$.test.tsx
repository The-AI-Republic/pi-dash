// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// What this suite owns is the mount: the splat renders the not-found
// screen itself, with no guard between the route and the screen.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
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

import { Route } from "./$.js";

describe("god-mode catch-all route", () => {
  it("renders the not-found screen for unknown console addresses", async () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("god-mode catch-all route has no component");
    const rootRoute = createRootRoute({ component: () => <Outlet /> });
    const splatRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/god-mode/$",
      component: Component,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([splatRoute]),
      history: createMemoryHistory({ initialEntries: ["/god-mode/no-such-place/"] }),
      trailingSlash: "preserve",
    });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("admin-not-found")).toBeInTheDocument());
    expect(screen.queryByTestId("admin-loader")).not.toBeInTheDocument();
    expect((await screen.findByRole("link", { name: "Go to general settings" })).getAttribute("href")).toBe(
      "/god-mode/general/"
    );
  });
});
