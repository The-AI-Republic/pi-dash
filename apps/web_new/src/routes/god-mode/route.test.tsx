// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// What this suite owns is the mount: web builds pass the gate and
// render the console outlet; desktop builds never mount /god-mode.
import { render, screen } from "@testing-library/react";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  isNotFound,
  RouterProvider,
} from "@tanstack/react-router";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const platformHarness = vi.hoisted(() => ({ kind: "web" }));

vi.mock("../../core/platform/index.js", () => ({
  get platform() {
    return platformHarness;
  },
}));

import { Route } from "./route.js";

describe("god-mode web-only gate", () => {
  afterEach(() => {
    platformHarness.kind = "web";
  });

  it("passes web builds through to the console", () => {
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("god-mode layout route has no beforeLoad");
    expect(beforeLoad({} as never)).toEqual({});
  });

  it("throws not-found on desktop builds", () => {
    platformHarness.kind = "desktop";
    const beforeLoad = Route.options.beforeLoad;
    if (!beforeLoad) throw new Error("god-mode layout route has no beforeLoad");
    let thrown: unknown;
    try {
      beforeLoad({} as never);
    } catch (error) {
      thrown = error;
    }
    expect(isNotFound(thrown)).toBe(true);
  });

  it("renders the console outlet", async () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("god-mode layout route has no component");
    const rootRoute = createRootRoute({ component: Component });
    const childRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/",
      component: () => <div data-testid="console-page">console</div>,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([childRoute]),
      history: createMemoryHistory({ initialEntries: ["/"] }),
    });
    render(<RouterProvider router={router} />);
    expect(await screen.findByTestId("console-page")).toHaveTextContent("console");
  });
});
