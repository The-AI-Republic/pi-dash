// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings tab address (parity/specs/profile.md §5, PROF-009): a known
// tab renders through the seam, an unknown one forwards to the default
// tab with the query kept.
import { cleanup, screen } from "@testing-library/react";
import { isRedirect, Outlet, type RouteComponent } from "@tanstack/react-router";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { registerProfileSettingsPage, resetProfileSettingsPages } from "../../../features/settings/lib/registry.js";
import { renderProfileSettingsRoute, resetProfileRouteHarness } from "../-routeHarness.js";
import { Route as IndexRoute, defaultTabHref } from "./index.js";
import { Route as TabRoute, unknownTabHref } from "./$tab.js";

afterEach(() => {
  cleanup();
  resetProfileSettingsPages();
  resetProfileRouteHarness();
});

function transport() {
  return async (url: string) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify({ csrf_token: "csrf" }),
  });
}

function thrownFrom(run: () => unknown): unknown {
  try {
    run();
  } catch (error) {
    return error;
  }
  return null;
}

describe("bare settings address", () => {
  it("forwards to the general tab", () => {
    expect(defaultTabHref("")).toBe("/settings/profile/general/");
  });

  it("keeps the query string", () => {
    expect(defaultTabHref("?tab=all&page=2")).toBe("/settings/profile/general/?tab=all&page=2");
    expect(defaultTabHref("tab=all")).toBe("/settings/profile/general/?tab=all");
  });

  it("redirects from the route's loader", () => {
    const thrown = thrownFrom(() => IndexRoute.options.beforeLoad?.({ location: { searchStr: "?tab=all" } } as never));
    expect(isRedirect(thrown)).toBe(true);
    expect((thrown as { options: { href: string } }).options.href).toBe("/settings/profile/general/?tab=all");
  });
});

describe("unknown tab address", () => {
  it("forwards to the general tab with the query kept", () => {
    expect(unknownTabHref("?tab=all")).toBe("/settings/profile/general/?tab=all");
  });

  it("passes a known tab through its loader", () => {
    expect(
      TabRoute.options.beforeLoad?.({ params: { tab: "security" }, location: { searchStr: "" } } as never)
    ).toBeUndefined();
  });

  it("forwards an unknown tab from its loader", () => {
    const thrown = thrownFrom(() =>
      TabRoute.options.beforeLoad?.({ params: { tab: "not-a-tab" }, location: { searchStr: "?x=1" } } as never)
    );
    expect(isRedirect(thrown)).toBe(true);
    expect((thrown as { options: { href: string } }).options.href).toBe("/settings/profile/general/?x=1");
  });

  it("renders the registered body for a known tab", async () => {
    registerProfileSettingsPage("security", () => <p>security body</p>);
    renderProfileSettingsRoute({
      layoutComponent: () => <Outletish />,
      leafComponent: TabRoute.options.component as RouteComponent,
      leafPath: "$tab",
      initialPath: "/settings/profile/security/",
      transport: transport(),
    });
    expect(await screen.findByText("security body")).toBeInTheDocument();
  });
});

function Outletish(): React.ReactElement {
  return <Outlet />;
}
