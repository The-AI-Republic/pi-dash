// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings tab address (parity/specs/profile.md §5, PROF-009): a known
// tab renders through the seam, an unknown one forwards to the default
// tab with the query kept.
import { QueryClient } from "@tanstack/react-query";
import { cleanup, screen } from "@testing-library/react";
import { isRedirect, Outlet, type RouteComponent } from "@tanstack/react-router";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient } from "../../../core/api/client.js";
import { registerProfileSettingsPage, resetProfileSettingsPages } from "../../../features/settings/lib/registry.js";
import { renderProfileSettingsRoute, resetProfileRouteHarness } from "../-routeHarness.js";
import { defaultTabHref, Route as IndexRoute } from "./index.js";
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

  it("passes a known tab through its loader", async () => {
    await expect(
      TabRoute.options.beforeLoad?.({
        context: guardedContext(instanceConfig(true)),
        params: { tab: "security" },
        location: { searchStr: "" },
      } as never)
    ).resolves.toBeUndefined();
  });

  it("forwards an unknown tab from its loader", async () => {
    const thrown = await Promise.resolve()
      .then(() =>
        TabRoute.options.beforeLoad?.({
          context: guardedContext(instanceConfig(true)),
          params: { tab: "not-a-tab" },
          location: { searchStr: "?x=1" },
        } as never)
      )
      .then(
        () => null,
        (error: unknown) => error
      );
    expect(isRedirect(thrown)).toBe(true);
    expect((thrown as { options: { href: string } }).options.href).toBe("/settings/profile/general/?x=1");
  });

  it("forwards the password address on a cloud-managed instance (CLOUD-035)", async () => {
    const context = guardedContext(instanceConfig(false));
    const thrown = await TabRoute.options
      .beforeLoad?.({
        context,
        params: { tab: "security" },
        location: { searchStr: "?x=1" },
      } as never)
      .then(
        () => null,
        (error: unknown) => error
      );
    expect(isRedirect(thrown)).toBe(true);
    expect((thrown as { options: { href: string } }).options.href).toBe("/settings/profile/general/?x=1");
  });

  it("serves the password address on a self-managed instance", async () => {
    const context = guardedContext(instanceConfig(true));
    await expect(
      TabRoute.options.beforeLoad?.({ context, params: { tab: "security" }, location: { searchStr: "" } } as never)
    ).resolves.toBeUndefined();
  });

  it("serves a tab no instance withholds without reading the instance", async () => {
    const getJson = vi.fn();
    const context = { queryClient: new QueryClient(), apiClient: { getJson } as never };
    await expect(
      TabRoute.options.beforeLoad?.({
        context,
        params: { tab: "ai-assistant" },
        location: { searchStr: "" },
      } as never)
    ).resolves.toBeUndefined();
    expect(getJson).not.toHaveBeenCalled();
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

function guardedContext(config: Record<string, unknown>) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const apiClient = createAppClient("https://api.test", async (url: string) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify({ config }),
  }));
  return { queryClient, apiClient };
}

/** The full instance row the contract parses; only the marker varies. */
function instanceConfig(selfManaged: boolean): Record<string, unknown> {
  return {
    enable_signup: true,
    is_workspace_creation_disabled: false,
    is_google_enabled: false,
    is_github_enabled: false,
    is_gitlab_enabled: false,
    is_gitea_enabled: false,
    is_magic_login_enabled: false,
    is_email_password_enabled: true,
    is_smtp_configured: false,
    is_self_managed: selfManaged,
    instance_changelog_url: "",
  };
}

function Outletish(): React.ReactElement {
  return <Outlet />;
}
