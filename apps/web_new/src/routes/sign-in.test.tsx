// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { createClient, type ApiClient } from "@pidash/api-client";
import { createMemoryHistory, createRootRoute, createRoute, createRouter, isRedirect } from "@tanstack/react-router";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

// Stand-in for the desktop screen: its internals belong to the auth area.
// What this suite owns is the web/desktop fork plus the guard rule.
vi.mock("../features/auth/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../features/auth/index.js")>()),
  DesktopSignInScreen: (props: { error?: string; errorDescription?: string }) => (
    <div
      data-testid="desktop-sign-in"
      data-error={props.error ?? ""}
      data-error-description={props.errorDescription ?? ""}
    />
  ),
}));

const platformHarness = vi.hoisted(() => ({ kind: "web" }));

vi.mock("../core/platform/index.js", () => ({
  get platform() {
    return platformHarness;
  },
}));

import { meQueryOptions, profileQueryOptions, sessionKeys } from "../core/session/queries.js";
import { redirectRetiredSignIn, Route } from "./sign-in.js";

interface ThrownRedirect extends Response {
  options: { to?: unknown; href?: unknown; search?: unknown };
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

function queryClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function unauthorizedClient(): ApiClient {
  return createClient({
    baseUrl: "https://api.test",
    transport: async (url) => ({
      status: 401,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify({ detail: "signed out" }),
    }),
  });
}

function seedSession(client: QueryClient, apiClient: ApiClient, onboarded: boolean) {
  client.setQueryData(meQueryOptions(apiClient).queryKey, { id: "u1" } as never);
  client.setQueryData(profileQueryOptions(apiClient).queryKey, { is_onboarded: onboarded } as never);
}

async function load(
  client: QueryClient,
  apiClient: ApiClient,
  search: Record<string, unknown>
): Promise<{ ok: unknown } | { thrown: unknown }> {
  const beforeLoad = Route.options.beforeLoad;
  if (!beforeLoad) throw new Error("missing beforeLoad");
  return beforeLoad({
    context: { queryClient: client, apiClient },
    location: { href: "/sign-in" },
    search,
  } as never).then(
    (value: unknown) => ({ ok: value }),
    (error: unknown) => ({ thrown: error })
  );
}

describe("sign-in guard", () => {
  afterEach(() => {
    platformHarness.kind = "web";
  });

  it("redirects bare to the entry on web builds", async () => {
    const outcome = await load(queryClient(), unauthorizedClient(), {
      error: "access_denied",
      error_description: "The provider refused.",
      next_path: "/acme",
    });
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: ThrownRedirect }).thrown.options).toMatchObject({ to: "/" });
    expect((outcome as { thrown: ThrownRedirect }).thrown.options.search).toBeUndefined();
  });

  it("renders the desktop screen for signed-out visitors on desktop", async () => {
    platformHarness.kind = "desktop";
    await expect(load(queryClient(), unauthorizedClient(), {})).resolves.toEqual({ ok: {} });
  });

  it("forwards signed-in members to their landing on desktop", async () => {
    platformHarness.kind = "desktop";
    const client = queryClient();
    const apiClient = unauthorizedClient();
    seedSession(client, apiClient, true);
    const outcome = await load(client, apiClient, { nextPath: "/acme" });
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: ThrownRedirect }).thrown.options).toMatchObject({ href: "/acme" });
  });

  it("funnels unfinished members to onboarding on desktop", async () => {
    platformHarness.kind = "desktop";
    const client = queryClient();
    const apiClient = unauthorizedClient();
    seedSession(client, apiClient, false);
    const outcome = await load(client, apiClient, {});
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: ThrownRedirect }).thrown.options).toMatchObject({ href: "/onboarding" });
  });

  it("lands members whose onboarding flags are all set even without the marker", async () => {
    platformHarness.kind = "desktop";
    const client = queryClient();
    const apiClient = unauthorizedClient();
    client.setQueryData(meQueryOptions(apiClient).queryKey, { id: "u1" } as never);
    client.setQueryData(profileQueryOptions(apiClient).queryKey, {
      is_onboarded: false,
      onboarding_step: {
        profile_complete: true,
        workspace_create: true,
        workspace_invite: true,
        workspace_join: true,
      },
    } as never);
    client.setQueryData(sessionKeys.settings, {
      workspace: { last_workspace_slug: "acme", fallback_workspace_slug: null },
    });
    client.setQueryData(sessionKeys.workspaces, [{ slug: "acme" }]);
    const outcome = await load(client, apiClient, {});
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: ThrownRedirect }).thrown.options).toMatchObject({ href: "/acme" });
  });
});

describe("DesktopSignInRouteScreen", () => {
  afterEach(() => {
    platformHarness.kind = "web";
    vi.restoreAllMocks();
  });

  it("renders the desktop sign-in screen on desktop builds", () => {
    platformHarness.kind = "desktop";
    vi.spyOn(Route, "useSearch").mockReturnValue({ error: "access_denied", errorDescription: "The provider refused." });
    const Component = Route.options.component;
    if (!Component) throw new Error("sign-in route has no component");
    render(<Component />);
    const screen_ = screen.getByTestId("desktop-sign-in");
    expect(screen_).toHaveAttribute("data-error", "access_denied");
    expect(screen_).toHaveAttribute("data-error-description", "The provider refused.");
  });

  it("renders nothing on web builds", () => {
    vi.spyOn(Route, "useSearch").mockReturnValue({});
    const Component = Route.options.component;
    if (!Component) throw new Error("sign-in route has no component");
    const { container } = render(<Component />);
    expect(container).toBeEmptyDOMElement();
  });
});
