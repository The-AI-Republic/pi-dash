// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { createClient, type Transport } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const runtime = vi.hoisted(() => ({ client: null as unknown, queryClient: null as unknown }));
vi.mock("../api/client.js", () => ({ spaceRuntime: runtime }));

import { describeSpaceAuthCode } from "../auth/spaceAuthErrors.js";
import type { SpaceLandingSearch } from "../lib/returnPath.js";
import {
  LandingGate,
  SPACE_GUIDANCE_HEADING,
  SPACE_GUIDANCE_LINE,
  SPACE_LANDING_TITLE,
  spaceLandingCapabilities,
} from "./LandingGate.js";

const UUID = "11111111-1111-4111-8111-111111111111";

function reply(status: number, body: unknown): Awaited<ReturnType<Transport>> {
  return {
    status,
    url: "",
    headers: { get: () => "application/json" },
    text: () => Promise.resolve(JSON.stringify(body)),
  };
}

const ME = {
  id: UUID,
  email: "ada@example.com",
  username: "ada",
  first_name: "Ada",
  last_name: "Lovelace",
  display_name: "Ada Lovelace",
  avatar: "",
  avatar_url: null,
  cover_image: null,
  cover_image_url: null,
  date_joined: "2026-01-01T00:00:00Z",
  user_timezone: "UTC",
  is_active: true,
  is_bot: false,
  is_email_verified: true,
  is_password_autoset: false,
  last_login_medium: "password",
  last_login_time: null,
};

function instanceWith(config: Record<string, unknown>) {
  return {
    config: {
      enable_signup: true,
      is_workspace_creation_disabled: false,
      is_google_enabled: false,
      is_github_enabled: false,
      is_gitlab_enabled: false,
      is_gitea_enabled: false,
      is_magic_login_enabled: false,
      is_email_password_enabled: false,
      is_smtp_configured: false,
      is_self_managed: true,
      instance_changelog_url: "",
      ...config,
    },
    instance: {
      id: UUID,
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
      created_by: null,
      updated_by: null,
      deleted_at: null,
      instance_name: "Pi Dash",
      whitelist_emails: null,
      instance_id: "instance-1",
      current_version: "1.0.0",
      latest_version: null,
      edition: "oss",
      domain: "example.test",
      last_checked_at: "2026-01-01T00:00:00Z",
      namespace: null,
      is_telemetry_enabled: true,
      is_support_required: false,
      is_setup_done: true,
      is_signup_screen_visited: false,
      is_verified: false,
      is_test: false,
      is_current_version_deprecated: false,
      workspaces_exist: true,
    },
  };
}

function transportFor(me: { status: number; body: unknown }, instance: { status: number; body: unknown }): Transport {
  return async (url) => {
    if (url.includes("/api/users/me/")) return reply(me.status, me.body);
    if (url.includes("/api/instances/")) return reply(instance.status, instance.body);
    if (url.includes("/auth/get-csrf-token/")) return reply(200, { csrf_token: "csrf-test" });
    throw new Error(`unexpected request ${url}`);
  };
}

function renderGate(
  search: SpaceLandingSearch,
  transport: Transport,
  slots?: { account?: React.ReactNode; badge?: React.ReactNode }
): void {
  runtime.client = createClient({ baseUrl: "https://api.test", transport });
  runtime.queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={runtime.queryClient as QueryClient}>
      <LandingGate search={search} account={slots?.account} badge={slots?.badge} />
    </QueryClientProvider>
  );
}

function providerHref(provider: string): string | null | undefined {
  return screen
    .getByTestId("space-auth-providers")
    .querySelector(`a[data-provider="${provider}"]`)
    ?.getAttribute("href");
}

afterEach(() => {
  document.title = "";
  for (const tag of [...document.head.querySelectorAll("meta")]) tag.remove();
  vi.restoreAllMocks();
});

describe("spaceLandingCapabilities", () => {
  it("maps every instance flag onto the card's capabilities", () => {
    const bootstrap = instanceWith({
      is_smtp_configured: true,
      is_magic_login_enabled: true,
      is_email_password_enabled: false,
      is_google_enabled: true,
      is_github_enabled: false,
      is_gitlab_enabled: true,
      is_gitea_enabled: false,
    });
    expect(spaceLandingCapabilities(bootstrap)).toEqual({
      smtpConfigured: true,
      magicLoginEnabled: true,
      emailPasswordEnabled: false,
      googleEnabled: true,
      githubEnabled: false,
      gitlabEnabled: true,
      giteaEnabled: false,
    });
  });

  it("disables every method without a bootstrap", () => {
    const off = {
      smtpConfigured: false,
      magicLoginEnabled: false,
      emailPasswordEnabled: false,
      googleEnabled: false,
      githubEnabled: false,
      gitlabEnabled: false,
      giteaEnabled: false,
    };
    expect(spaceLandingCapabilities(null)).toEqual(off);
    expect(spaceLandingCapabilities(undefined)).toEqual(off);
  });
});

describe("LandingGate", () => {
  it("shows the branded loader while the session resolves", () => {
    const hanging: Transport = async () => {
      await new Promise<never>(() => {});
      return reply(500, { detail: "unreachable" });
    };
    renderGate({}, hanging);
    expect(screen.getByTestId("space-landing")).toHaveAttribute("role", "status");
    expect(screen.getByAltText("Pi Dash")).toHaveAttribute("src", "/icon.svg");
    expect(screen.getByText("Loading public boards")).toBeInTheDocument();
  });

  it("keeps the loader until the instance settles, even with the viewer known", async () => {
    const transport: Transport = async (url) => {
      if (url.includes("/api/users/me/")) return reply(401, { detail: "no session" });
      await new Promise<never>(() => {});
      return reply(500, { detail: "unreachable" });
    };
    renderGate({}, transport);
    await waitFor(() =>
      expect((runtime.queryClient as QueryClient).getQueryState(["spaces", "viewer"])?.status).toBe("success")
    );
    expect(screen.getByTestId("space-landing")).toHaveAttribute("role", "status");
    expect(screen.queryByTestId("space-auth-card")).not.toBeInTheDocument();
    expect(screen.queryByTestId("space-landing-guidance")).not.toBeInTheDocument();
  });

  it("gives anonymous visitors the auth card with prefill and coded failures", async () => {
    renderGate(
      { email: "ada@example.com", errorCode: "5075" },
      transportFor({ status: 401, body: { detail: "no session" } }, { status: 200, body: instanceWith({}) })
    );
    const card = await screen.findByTestId("space-auth-card");
    expect(card).toHaveAttribute("data-step", "email");
    expect(screen.getByLabelText("Email address")).toHaveValue("ada@example.com");
    expect(screen.getByTestId("space-auth-banner")).toHaveTextContent(
      describeSpaceAuthCode("5075")?.message ?? "missing mapping"
    );
    expect(document.title).toBe(SPACE_LANDING_TITLE);
    expect(document.head.querySelector('meta[name="robots"]')).toHaveAttribute("content", "noindex, nofollow");
  });

  it("carries a valid return path through the card and drops hostile ones", async () => {
    const withProvider = { status: 200, body: instanceWith({ is_google_enabled: true }) };
    const anonymous = { status: 401, body: { detail: "no session" } };
    renderGate({ nextPath: "/issues/anchor-1?board=kanban" }, transportFor(anonymous, withProvider));
    await screen.findByTestId("space-auth-card");
    expect(providerHref("google")).toBe("/auth/google/?next_path=%2Fissues%2Fanchor-1%3Fboard%3Dkanban");
  });

  it("drops a hostile return path before it reaches the card", async () => {
    renderGate(
      { nextPath: "https://evil.example/issues/x" },
      transportFor(
        { status: 401, body: { detail: "no session" } },
        { status: 200, body: instanceWith({ is_google_enabled: true }) }
      )
    );
    await screen.findByTestId("space-auth-card");
    expect(providerHref("google")).toBe("/auth/google/");
  });

  it("renders the card with every method off when the bootstrap is missing", async () => {
    renderGate(
      {},
      transportFor({ status: 401, body: { detail: "no session" } }, { status: 500, body: { detail: "down" } })
    );
    const card = await screen.findByTestId("space-auth-card");
    expect(card).toHaveAttribute("data-step", "email");
    expect(screen.queryByTestId("space-auth-providers")).not.toBeInTheDocument();
  });

  it("gives signed-in visitors the paste-a-link guidance with their slots", async () => {
    renderGate({}, transportFor({ status: 200, body: ME }, { status: 200, body: instanceWith({}) }), {
      account: <span data-testid="slot-account" />,
      badge: <span data-testid="slot-badge" />,
    });
    const guidance = await screen.findByTestId("space-landing-guidance");
    expect(guidance).toHaveTextContent(SPACE_GUIDANCE_HEADING);
    expect(guidance).toHaveTextContent(SPACE_GUIDANCE_LINE);
    expect(screen.getByRole("heading", { name: SPACE_GUIDANCE_HEADING })).toBeInTheDocument();
    expect(screen.getByTestId("slot-account")).toBeInTheDocument();
    expect(screen.getByTestId("slot-badge")).toBeInTheDocument();
    expect(screen.queryByTestId("space-auth-card")).not.toBeInTheDocument();
    expect(document.title).toBe(SPACE_LANDING_TITLE);
    expect(document.head.querySelector('meta[name="robots"]')).toHaveAttribute("content", "noindex, nofollow");
  });

  it("keeps signed-in visitors on the guidance view when the return path is hostile", async () => {
    renderGate(
      { nextPath: "//evil.example/issues/x" },
      transportFor({ status: 200, body: ME }, { status: 200, body: instanceWith({}) })
    );
    expect(await screen.findByTestId("space-landing-guidance")).toBeInTheDocument();
  });

  it("never navigates to a return path naming the landing itself", async () => {
    const router = renderGateInRouter(
      { nextPath: "/" },
      transportFor({ status: 200, body: ME }, { status: 200, body: instanceWith({}) })
    );
    expect(await screen.findByTestId("space-landing-guidance")).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/spaces/");
  });

  it("returns signed-in visitors with a valid return path to the board", async () => {
    const router = renderGateInRouter(
      { nextPath: "/issues/anchor-1?board=kanban" },
      transportFor({ status: 200, body: ME }, { status: 200, body: instanceWith({}) })
    );
    await waitFor(() => expect(router.state.location.pathname).toBe("/spaces/issues/anchor-1"));
    expect(router.state.location.search).toMatchObject({ board: "kanban" });
    expect(await screen.findByTestId("redirect-board")).toBeInTheDocument();
  });
});

function renderGateInRouter(search: SpaceLandingSearch, transport: Transport) {
  runtime.client = createClient({ baseUrl: "https://api.test", transport });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  runtime.queryClient = queryClient;
  const rootRoute = createRootRoute({
    component: () => (
      <QueryClientProvider client={queryClient}>
        <Outlet />
      </QueryClientProvider>
    ),
  });
  const landingRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/spaces/",
    component: () => <LandingGate search={search} />,
  });
  const boardRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/spaces/issues/$anchor",
    component: () => <div data-testid="redirect-board" />,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([landingRoute, boardRoute]),
    history: createMemoryHistory({ initialEntries: ["/spaces/"] }),
  });
  render(<RouterProvider router={router} />);
  return router;
}
