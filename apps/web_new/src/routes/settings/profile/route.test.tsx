// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Personal-settings layout (parity/specs/profile.md §5, PROF-009/010):
// the shell over the tab outlet, the tab read from the address, the
// sidebar shortcuts, and the unknown-tab fallback.
import { QueryClient } from "@tanstack/react-query";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { isRedirect, type RouteComponent } from "@tanstack/react-router";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../features/settings/pageEntries.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../features/settings/pageEntries.js")>();
  return { ...actual, useProfileSettingsSessionGuard: vi.fn() };
});

import { resetProfileSettingsPages } from "../../../features/settings/lib/registry.js";
import { useProfileSettingsSessionGuard } from "../../../features/settings/pageEntries.js";
import { resetProfileRouteHarness, renderProfileSettingsRoute } from "../-routeHarness.js";
import { createAppClient } from "../../../core/api/client.js";
import { useSessionStore } from "../../../core/session/store.js";
import { loadProfileSettingsLayout } from "../../../features/settings/loadProfileSettings.js";
import { Route as TokensRoute } from "./api-tokens.js";
import { Route, tabFromPathname } from "./route.js";

const Layout = Route.options.component;
if (Layout === undefined) throw new Error("missing settings layout component");
const LayoutComponent: RouteComponent = Layout;

const PROFILE_ROW = {
  id: "223e4567-e89b-12d3-a456-426614174000",
  user: "123e4567-e89b-12d3-a456-426614174000",
  theme: {},
  is_app_rail_docked: true,
  is_tour_completed: false,
  onboarding_step: {},
  use_case: null,
  role: null,
  is_onboarded: true,
  last_workspace_id: null,
  billing_address_country: "US",
  billing_address: null,
  has_billing_address: false,
  company_name: "",
  notification_view_mode: "default",
  is_smooth_cursor_enabled: false,
  is_mobile_onboarded: false,
  mobile_onboarding_step: {},
  mobile_timezone_auto_set: false,
  language: "en",
  start_of_the_week: 0,
  goals: {},
  background_color: "",
  is_navigation_tour_completed: false,
  has_marketing_email_consent: false,
  is_subscribed_to_changelog: false,
  product_tour: {},
  settings: {},
  created_at: "2024-01-01T00:00:00Z",
  updated_at: "2024-01-01T00:00:00Z",
};

function transport(): Parameters<typeof renderProfileSettingsRoute>[0]["transport"] {
  return async (url) => {
    const headers = { get: () => null };
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers, text: async () => JSON.stringify({ csrf_token: "csrf" }) };
    }
    if (url.endsWith("/api/users/api-tokens/")) {
      return { status: 200, url, headers, text: async () => JSON.stringify([]) };
    }
    if (url.endsWith("/api/users/me/")) {
      return {
        status: 200,
        url,
        headers,
        text: async () =>
          JSON.stringify({
            id: "123e4567-e89b-12d3-a456-426614174000",
            email: "ada@example.com",
            username: "ada",
            first_name: "Ada",
            last_name: "Lovelace",
            display_name: "ada",
            avatar: "",
            avatar_url: null,
            cover_image: null,
            cover_image_url: null,
            date_joined: "2024-01-01T00:00:00Z",
            user_timezone: "UTC",
            is_active: true,
            is_bot: false,
            is_email_verified: true,
            is_password_autoset: false,
            last_login_medium: "password",
            last_login_time: null,
          }),
      };
    }
    if (url.includes("/api/users/me/profile/")) {
      return { status: 200, url, headers, text: async () => JSON.stringify(PROFILE_ROW) };
    }
    if (url.endsWith("/api/users/me/workspaces/")) {
      return {
        status: 200,
        url,
        headers,
        text: async () =>
          JSON.stringify([{ id: "ws-1", name: "Acme", slug: "acme", logo_url: null, total_members: 1, role: 20 }]),
      };
    }
    return { status: 404, url, headers, text: async () => JSON.stringify({ detail: "not found" }) };
  };
}

function setupHarness(routeTransport = transport()) {
  const apiClient = createAppClient("https://api.test", routeTransport);
  return { apiClient, queryClient: new QueryClient({ defaultOptions: { queries: { retry: false } } }) };
}

function renderAt(path: string, tab: string) {
  return renderProfileSettingsRoute({
    layoutComponent: LayoutComponent,
    leafComponent: () => <p data-testid="leaf">{`leaf ${tab}`}</p>,
    leafPath: "$tab",
    initialPath: path,
    transport: transport(),
  });
}

afterEach(() => {
  cleanup();
  resetProfileSettingsPages();
  resetProfileRouteHarness();
  // The visitor-gate test's 401 leaves the session expired; the tokens tab
  // (and the layout guard) react to it, so each test starts signed in.
  useSessionStore.setState({ status: "unknown", returnUrl: null });
  document.title = "";
});

describe("profile settings layout", () => {
  it("reads the active tab from the address and renders its body", async () => {
    renderAt("/settings/profile/security/", "security");
    expect(await screen.findByTestId("profile-settings-shell")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Security" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByTestId("leaf")).toHaveTextContent("leaf security");
    await waitFor(() => expect(document.title).toBe("Security · Profile settings"));
  });

  it("warms the workspace list for the shortcut sidebar", async () => {
    const requested: string[] = [];
    const base = transport();
    const { queryClient, apiClient } = setupHarness(async (url, init) => {
      requested.push(url);
      return base(url, init);
    });
    await loadProfileSettingsLayout({ queryClient, apiClient, href: "/settings/profile/general/" });
    expect(requested.filter((url) => url.endsWith("/api/users/me/workspaces/"))).toHaveLength(1);
  });

  it("keeps the page usable when the shortcut warm fails", async () => {
    const requested: string[] = [];
    const base = transport();
    const { queryClient, apiClient } = setupHarness(async (url, init) => {
      requested.push(url);
      if (url.endsWith("/api/users/me/workspaces/")) {
        return { status: 500, url, headers: { get: () => null }, text: async () => "{}" };
      }
      return base(url, init);
    });
    await expect(
      loadProfileSettingsLayout({ queryClient, apiClient, href: "/settings/profile/general/" })
    ).resolves.toEqual({ workspaces: [] });
  });

  it("gates the layout for visitors before warming the list", async () => {
    const requested: string[] = [];
    const base = transport();
    const { queryClient, apiClient } = setupHarness(async (url, init) => {
      if (url.endsWith("/api/users/me/")) {
        return { status: 401, url, headers: { get: () => null }, text: async () => "{}" };
      }
      requested.push(url);
      return base(url, init);
    });
    const thrown = await loadProfileSettingsLayout({
      queryClient,
      apiClient,
      href: "/settings/profile/general/",
    }).then(
      () => null,
      (error: unknown) => error
    );
    expect(isRedirect(thrown)).toBe(true);
    expect(requested.some((url) => url.endsWith("/api/users/me/workspaces/"))).toBe(false);
  });

  it("falls back to the general tab for an unknown address segment", async () => {
    renderAt("/settings/profile/not-a-tab/", "not-a-tab");
    expect(await screen.findByTestId("profile-settings-shell")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("link", { name: "Profile" })).toHaveAttribute("aria-current", "page"));
  });

  it("marks no tab active on the bare layout address", async () => {
    renderAt("/settings/profile/", "");
    expect(await screen.findByTestId("profile-settings-shell")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Profile" })).toHaveAttribute("aria-current", "page");
  });

  it("navigates in-app to a tab link", async () => {
    const { router } = renderAt("/settings/profile/general/", "general");
    await screen.findByTestId("profile-settings-shell");
    fireEvent.click(screen.getByRole("link", { name: "Personal Access Tokens" }));
    await waitFor(() =>
      expect(screen.getByRole("link", { name: "Personal Access Tokens" })).toHaveAttribute("aria-current", "page")
    );
    // The shell hands a plain href to the layout, which routes it without a
    // document load (the memory history normalizes the link's slash away).
    await waitFor(() => expect(router.state.location.pathname).toBe("/settings/profile/api-tokens"));
  });

  it("mounts the tokens leaf inside the shell through the route tree", async () => {
    renderProfileSettingsRoute({
      layoutComponent: LayoutComponent,
      leafComponent: TokensRoute.options.component as RouteComponent,
      leafPath: "api-tokens",
      initialPath: "/settings/profile/api-tokens",
      transport: transport(),
    });
    expect(await screen.findByTestId("profile-settings-shell")).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "Personal Access Tokens" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Personal Access Tokens" })).toHaveAttribute("aria-current", "page");
  });

  it("runs the session-expiry guard outside the app shell", async () => {
    renderAt("/settings/profile/general/", "general");
    await screen.findByTestId("profile-settings-shell");
    expect(useProfileSettingsSessionGuard).toHaveBeenCalled();
  });
});

describe("tabFromPathname", () => {
  it("names the tab in a settings address", () => {
    expect(tabFromPathname("/settings/profile/security/")).toBe("security");
    expect(tabFromPathname("/settings/profile/api-tokens")).toBe("api-tokens");
  });

  it("answers null for the bare layout address and for other paths", () => {
    expect(tabFromPathname("/settings/profile/")).toBeNull();
    expect(tabFromPathname("/settings/profile")).toBeNull();
    expect(tabFromPathname("/acme/settings/profile/general/")).toBeNull();
  });
});
