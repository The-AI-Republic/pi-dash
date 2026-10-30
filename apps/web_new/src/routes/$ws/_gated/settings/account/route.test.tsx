// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Account-settings layout (PROF-009/010/026; main#670/#675): the tab comes
// from the address, the navigator is the middle panel beside the body, the
// Workspace section lists every workspace with the viewed one open, the
// cloud instance withholds the password tab, and the title follows the tab.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
  type RouteComponent,
} from "@tanstack/react-router";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { RouterContext } from "../../../../__root.js";
import { createAppClient, resetAppClient } from "../../../../../core/api/client.js";
import { ROLE_ADMIN, ROLE_MEMBER, useSessionStore } from "../../../../../core/session/index.js";
import { resetProfileSettingsPages } from "../../../../../features/settings/lib/registry.js";
import { Route, accountTabFromPathname } from "./route.js";

const Layout = Route.options.component;
if (Layout === undefined) throw new Error("missing account settings layout component");
const LayoutComponent: RouteComponent = Layout;

const WORKSPACES = [
  {
    id: "123e4567-e89b-12d3-a456-426614174000",
    name: "Acme",
    slug: "acme",
    logo_url: null,
    total_members: 2,
    role: ROLE_MEMBER,
  },
  {
    id: "223e4567-e89b-12d3-a456-426614174000",
    name: "Globex",
    slug: "globex",
    logo_url: null,
    total_members: 2,
    role: ROLE_ADMIN,
  },
];

function transport(options: { selfManaged?: boolean } = {}) {
  return async (url: string) => {
    const ok = (body: unknown) => ({
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    });
    if (url.endsWith("/auth/get-csrf-token/")) return ok({ csrf_token: "csrf" });
    if (url.endsWith("/api/users/me/workspaces/")) return ok(WORKSPACES);
    if (url.endsWith("/api/instances/")) {
      return ok({
        config: {
          enable_signup: true,
          is_workspace_creation_disabled: false,
          is_google_enabled: false,
          is_github_enabled: false,
          is_gitlab_enabled: false,
          is_gitea_enabled: false,
          is_magic_login_enabled: false,
          is_email_password_enabled: true,
          is_smtp_configured: false,
          is_self_managed: options.selfManaged ?? true,
          instance_changelog_url: "",
        },
      });
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  };
}

function renderAt(initialPath: string, selfManaged = true) {
  const apiClient = createAppClient("https://api.test", transport({ selfManaged }));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const rootRoute = createRootRouteWithContext<RouterContext>()({});
  const wsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/$ws", component: () => <Outlet /> });
  const gatedRoute = createRoute({ getParentRoute: () => wsRoute, id: "_gated", component: () => <Outlet /> });
  const settingsRoute = createRoute({
    getParentRoute: () => gatedRoute,
    path: "settings",
    component: () => <Outlet />,
  });
  const accountRoute = createRoute({
    getParentRoute: () => settingsRoute,
    path: "account",
    component: LayoutComponent,
  });
  const tabRoute = createRoute({
    getParentRoute: () => accountRoute,
    path: "$tab",
    component: () => <p data-testid="tab-body">tab body</p>,
  });
  const indexRoute = createRoute({
    getParentRoute: () => accountRoute,
    path: "/",
    component: () => <p data-testid="tab-body">bare body</p>,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([
      wsRoute.addChildren([
        gatedRoute.addChildren([settingsRoute.addChildren([accountRoute.addChildren([tabRoute, indexRoute])])]),
      ]),
    ]),
    context: { queryClient, apiClient },
    history: createMemoryHistory({ initialEntries: [initialPath] }),
  });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { router, ...view };
}

afterEach(() => {
  cleanup();
  resetProfileSettingsPages();
  resetAppClient();
  useSessionStore.setState({ status: "unknown", returnUrl: null });
  document.title = "";
  vi.restoreAllMocks();
});

describe("account settings layout", () => {
  it("reads the tab from the address and renders the body beside the navigator", async () => {
    renderAt("/acme/settings/account/security/");
    expect(await screen.findByTestId("profile-settings-shell")).toBeInTheDocument();
    expect(screen.getByTestId("profile-settings-tab-security")).toHaveAttribute("aria-current", "page");
    expect(screen.getByTestId("tab-body")).toBeInTheDocument();
    expect(screen.getByTestId("profile-settings-sidebar")).toBeInTheDocument();
    await waitFor(() => expect(document.title).toBe("Security · Profile settings"));
  });

  it("names the page generically while the address is still the bare collection", async () => {
    renderAt("/acme/settings/account/");
    expect(await screen.findByTestId("profile-settings-shell")).toBeInTheDocument();
    expect(screen.getByTestId("profile-settings-tab-general")).toHaveAttribute("aria-current", "page");
    expect(screen.getByTestId("account-settings-mobile-crumb")).toHaveTextContent("Profile settings");
    await waitFor(() => expect(document.title).toBe("Profile settings"));
  });

  it("opens the workspace in the address and leaves the others closed", async () => {
    renderAt("/acme/settings/account/general/");
    expect(await screen.findByTestId("account-settings-workspace-toggle-acme")).toHaveAttribute(
      "aria-expanded",
      "true"
    );
    expect(screen.getByTestId("account-settings-workspace-toggle-globex")).toHaveAttribute("aria-expanded", "false");
  });

  it("keeps the password tab off a cloud-managed instance (CLOUD-035)", async () => {
    renderAt("/acme/settings/account/general/", false);
    await screen.findByTestId("profile-settings-shell");
    await waitFor(() => expect(screen.queryByTestId("profile-settings-tab-security")).not.toBeInTheDocument());
    expect(screen.getByTestId("profile-settings-tab-general")).toBeInTheDocument();
  });

  it("keeps the password tab on a self-managed instance", async () => {
    renderAt("/acme/settings/account/general/", true);
    await screen.findByTestId("profile-settings-shell");
    await waitFor(() => expect(screen.getByTestId("profile-settings-tab-security")).toBeInTheDocument());
  });

  it("lights no workspace entry while an account tab is open", async () => {
    renderAt("/acme/settings/account/general/");
    const general = await screen.findByTestId("account-settings-workspace-link-acme-general");
    expect(general).not.toHaveAttribute("aria-current");
  });

  it("tolerates a workspace list that has not arrived yet", async () => {
    renderAt("/acme/settings/account/general/");
    expect(await screen.findByTestId("profile-settings-shell")).toBeInTheDocument();
  });

  it("navigates in-app when a tab link is followed", async () => {
    const { router } = renderAt("/acme/settings/account/general/");
    await screen.findByTestId("profile-settings-shell");
    fireEvent.click(screen.getByTestId("profile-settings-tab-notifications"));
    // The memory history keeps the path without its trailing slash.
    await waitFor(() => expect(router.state.location.pathname).toBe("/acme/settings/account/notifications"));
    expect(screen.getByTestId("profile-settings-tab-notifications")).toHaveAttribute("aria-current", "page");
  });
});

describe("accountTabFromPathname", () => {
  it("names the tab in an account address", () => {
    expect(accountTabFromPathname("/acme/settings/account/security/", "acme")).toBe("security");
    expect(accountTabFromPathname("/acme/settings/account/api-tokens", "acme")).toBe("api-tokens");
  });

  it("answers null for the collection, unknown ids and other workspaces", () => {
    expect(accountTabFromPathname("/acme/settings/account/", "acme")).toBeNull();
    expect(accountTabFromPathname("/acme/settings/account/bogus", "acme")).toBeNull();
    expect(accountTabFromPathname("/globex/settings/account/security/", "acme")).toBeNull();
    expect(accountTabFromPathname("/acme/settings/members/", "acme")).toBeNull();
  });
});
