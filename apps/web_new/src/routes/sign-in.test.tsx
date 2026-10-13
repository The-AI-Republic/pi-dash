// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { createMemoryHistory, createRootRoute, createRoute, createRouter } from "@tanstack/react-router";
import { describe, expect, it } from "vitest";

import { redirectRetiredSignIn } from "./sign-in.js";

interface ThrownRedirect extends Response {
  options: { to?: unknown };
}

function catchRedirect(call: () => never): ThrownRedirect {
  try {
    call();
  } catch (error) {
    expect(error).toBeInstanceOf(Response);
    return error as ThrownRedirect;
  }
  throw new Error("expected the handler to redirect");
}

describe("redirectRetiredSignIn", () => {
  it("targets the home route", () => {
    expect(catchRedirect(redirectRetiredSignIn).options.to).toBe("/");
  });

  it("lands bare on / even with entry params", async () => {
    const rootRoute = createRootRoute();
    const homeRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/",
      component: () => null,
    });
    const legacyRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/sign-in",
      beforeLoad: redirectRetiredSignIn,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([homeRoute, legacyRoute]),
      history: createMemoryHistory({ initialEntries: ["/sign-in?email=a%40b.c&next_path=%2Facme"] }),
    });
    await router.load();
    expect(router.state.location.pathname).toBe("/");
    expect(router.state.location.search).toEqual({});
  });
});
