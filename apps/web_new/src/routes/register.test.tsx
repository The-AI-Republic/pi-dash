// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { createMemoryHistory, createRootRoute, createRoute, createRouter } from "@tanstack/react-router";
import { render } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { LEGACY_SIGN_UP_TARGET, LegacySignUpRedirect } from "./register.js";

describe("LegacySignUpRedirect", () => {
  const replace = vi.fn();

  beforeEach(() => {
    replace.mockClear();
    Object.defineProperty(globalThis.window, "location", {
      value: { replace },
      writable: true,
      configurable: true,
    });
  });

  it("forwards to the canonical card with its trailing slash", () => {
    expect(LEGACY_SIGN_UP_TARGET).toBe("/sign-up/");
    render(<LegacySignUpRedirect />);
    expect(replace).toHaveBeenCalledWith("/sign-up/");
  });

  it("keeps the trailing slash on the landing load", async () => {
    // Guards the full-document navigation above: the old app redirects
    // legacy sign-up addresses to /sign-up/ with the slash, so the
    // landing load must preserve it.
    const rootRoute = createRootRoute();
    const targetRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/sign-up",
      component: () => null,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([targetRoute]),
      history: createMemoryHistory({ initialEntries: ["/sign-up/"] }),
    });
    await router.load();
    expect(router.state.location.pathname).toBe("/sign-up/");
  });
});
