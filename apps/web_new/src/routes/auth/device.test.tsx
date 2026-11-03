// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { act, render, screen } from "@testing-library/react";
import { createClient, type ApiClient, type Transport } from "@pidash/api-client";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  isRedirect,
  RouterProvider,
} from "@tanstack/react-router";
import { showToast } from "@pidash/kit";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// The real router except navigation: the guard throws real redirects while
// the expired-redirect hook reads a stubbed location.
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useRouter: () => ({ state: { location: { href: "/auth/device" } }, navigate: vi.fn() }),
}));

// Stand-in for the auth card: its internals belong to the auth area. What
// this suite owns is the guard rule plus the search wiring.
vi.mock("../../features/auth/components/DeviceApprovalCard.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../features/auth/components/DeviceApprovalCard.js")>()),
  DeviceApprovalCard: ({ initialCode }: { initialCode?: string | undefined }) => (
    <div data-testid="device-card" data-initial-code={initialCode ?? ""} />
  ),
}));

import { meQueryOptions, profileQueryOptions } from "../../core/session/queries.js";
import { getToastManager } from "../../shared/shell/index.js";
import type { RouterContext } from "../__root.js";
import { DeviceScreen, parseDeviceSearch, Route } from "./device.js";

function throwingClient(): ApiClient {
  const transport: Transport = (url) => {
    throw new Error(`unexpected fetch ${url}`);
  };
  return createClient({ baseUrl: "https://api.test", transport });
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

function queryClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function seedSession(client: QueryClient, apiClient: ApiClient, onboarded: boolean) {
  client.setQueryData(meQueryOptions(apiClient).queryKey, { id: "u1" } as never);
  client.setQueryData(profileQueryOptions(apiClient).queryKey, { is_onboarded: onboarded } as never);
}

async function load(client: QueryClient, apiClient: ApiClient): Promise<{ ok: unknown } | { thrown: unknown }> {
  const beforeLoad = Route.options.beforeLoad;
  if (!beforeLoad) throw new Error("missing beforeLoad");
  return beforeLoad({ context: { queryClient: client, apiClient }, location: { href: "/auth/device" } } as never).then(
    (value: unknown) => ({ ok: value }),
    (error: unknown) => ({ thrown: error })
  );
}

describe("parseDeviceSearch", () => {
  it("carries the CLI code through", () => {
    expect(parseDeviceSearch({ code: "zzzz-yyyy" })).toEqual({ code: "zzzz-yyyy" });
  });

  it("falls back to absent without a usable code", () => {
    expect(parseDeviceSearch({})).toEqual({});
    expect(parseDeviceSearch({ code: "" })).toEqual({});
    expect(parseDeviceSearch({ code: 7 })).toEqual({});
  });
});

describe("device guard", () => {
  it("bounces signed-out visitors to the entry with the return path", async () => {
    const outcome = await load(queryClient(), unauthorizedClient());
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: { options: unknown } }).thrown.options).toMatchObject({
      to: "/",
      search: { next_path: "/auth/device" },
    });
  });

  it("renders for signed-in onboarded users", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    seedSession(client, apiClient, true);
    await expect(load(client, apiClient)).resolves.toEqual({ ok: {} });
  });

  it("funnels unfinished users to onboarding", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    seedSession(client, apiClient, false);
    const outcome = await load(client, apiClient);
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: { options: unknown } }).thrown.options).toMatchObject({ href: "/onboarding" });
  });

  it("fails closed toward onboarding when the profile cannot be read", async () => {
    const client = queryClient();
    const apiClient = unauthorizedClient();
    client.setQueryData(meQueryOptions(apiClient).queryKey, { id: "u1" } as never);
    const outcome = await load(client, apiClient);
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: { options: unknown } }).thrown.options).toMatchObject({ href: "/onboarding" });
  });
});

describe("DeviceScreen", () => {
  it("hands the CLI-carried code to the card", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    seedSession(client, apiClient, true);
    const validateSearch = Route.options.validateSearch;
    const beforeLoad = Route.options.beforeLoad;
    const component = Route.options.component;
    if (!validateSearch || !beforeLoad || !component) throw new Error("missing route options");
    const rootRoute = createRootRouteWithContext<RouterContext>()();
    const deviceRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/auth/device",
      validateSearch,
      beforeLoad,
      component,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([deviceRoute]),
      context: { queryClient: client, apiClient },
      history: createMemoryHistory({ initialEntries: ["/auth/device?code=zzzz-yyyy"] }),
    });
    render(<RouterProvider router={router} />);
    const card = await screen.findByTestId("device-card");
    expect(card).toHaveAttribute("data-initial-code", "zzzz-yyyy");
  });

  // Last: leaves a toast behind in the (per-file) manager on purpose.
  it("mounts the shared toast host", async () => {
    const rootRoute = createRootRouteWithContext<RouterContext>()();
    const deviceRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/auth/device",
      component: DeviceScreen,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([deviceRoute]),
      context: { queryClient: queryClient(), apiClient: throwingClient() },
      history: createMemoryHistory({ initialEntries: ["/auth/device"] }),
    });
    render(<RouterProvider router={router} />);
    await screen.findByTestId("device-card");
    act(() => {
      showToast(getToastManager(), { title: "Host is live" });
    });
    expect(await screen.findByText("Host is live")).toBeInTheDocument();
  });
});
