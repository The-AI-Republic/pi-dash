// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { act, render, screen } from "@testing-library/react";
import { createClient, type ApiClient, type Transport } from "@pidash/api-client";
import { isRedirect } from "@tanstack/react-router";
import { showToast } from "@pidash/kit";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// The real router except navigation: the guard throws real redirects while
// the expired-redirect hook reads a stubbed location.
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useRouter: () => ({ state: { location: { href: "/accounts/set-password" } }, navigate: vi.fn() }),
}));

// Stand-in for the auth card: its internals belong to the auth area.
// What this suite owns is the guard rule plus the screen wiring.
vi.mock("../../features/auth/components/SetPasswordCard.js", () => ({
  SetPasswordCard: () => <div data-testid="set-card" />,
}));

import { meQueryOptions, profileQueryOptions } from "../../core/session/queries.js";
import { getToastManager } from "../../shared/shell/index.js";
import { decideSetPasswordGate, Route, SetPasswordScreen } from "./set-password.js";

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

function beforeLoadOf(route: typeof Route) {
  const beforeLoad = route.options.beforeLoad;
  if (!beforeLoad) throw new Error("missing beforeLoad");
  return beforeLoad;
}

function seedMe(client: QueryClient, apiClient: ApiClient, isPasswordAutoset: boolean) {
  client.setQueryData(meQueryOptions(apiClient).queryKey, {
    id: "u1",
    is_password_autoset: isPasswordAutoset,
  } as never);
}

const ALL_STEP_FLAGS = {
  profile_complete: true,
  workspace_create: true,
  workspace_invite: true,
  workspace_join: true,
};

function seedProfile(
  client: QueryClient,
  apiClient: ApiClient,
  profile: { is_onboarded: boolean; onboarding_step?: Record<string, unknown> }
) {
  client.setQueryData(profileQueryOptions(apiClient).queryKey, profile as never);
}

async function load(client: QueryClient, apiClient: ApiClient): Promise<{ ok: unknown } | { thrown: unknown }> {
  return beforeLoadOf(Route)({
    context: { queryClient: client, apiClient },
    location: { href: "/accounts/set-password" },
  } as never).then(
    (value: unknown) => ({ ok: value }),
    (error: unknown) => ({ thrown: error })
  );
}

describe("decideSetPasswordGate", () => {
  it("renders passwordless accounts and sends set onboarded users away", () => {
    expect(decideSetPasswordGate({ passwordAutoset: true, onboarded: true })).toBe("render");
    expect(decideSetPasswordGate({ passwordAutoset: true, onboarded: false })).toBe("render");
    expect(decideSetPasswordGate({ passwordAutoset: false, onboarded: true })).toBe("away");
    expect(decideSetPasswordGate({ passwordAutoset: false, onboarded: false })).toBe("render");
    expect(decideSetPasswordGate({ passwordAutoset: false, onboarded: null })).toBe("render");
  });
});

describe("set-password guard", () => {
  it("bounces signed-out visitors to the entry with the return path", async () => {
    const outcome = await load(queryClient(), unauthorizedClient());
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: { options: unknown } }).thrown.options).toMatchObject({
      to: "/",
      search: { next_path: "/accounts/set-password" },
    });
  });

  it("renders for passwordless accounts", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    seedMe(client, apiClient, true);
    seedProfile(client, apiClient, { is_onboarded: false });
    await expect(load(client, apiClient)).resolves.toEqual({ ok: {} });
  });

  it("sends set onboarded users away", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    seedMe(client, apiClient, false);
    seedProfile(client, apiClient, { is_onboarded: true });
    const outcome = await load(client, apiClient);
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: { options: unknown } }).thrown.options).toMatchObject({ to: "/" });
  });

  it("sends away set users whose flags are all set even without the marker", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    seedMe(client, apiClient, false);
    seedProfile(client, apiClient, { is_onboarded: false, onboarding_step: { ...ALL_STEP_FLAGS } });
    const outcome = await load(client, apiClient);
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: { options: unknown } }).thrown.options).toMatchObject({ to: "/" });
  });

  it("renders for set users who are not onboarded yet", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    seedMe(client, apiClient, false);
    seedProfile(client, apiClient, { is_onboarded: false });
    await expect(load(client, apiClient)).resolves.toEqual({ ok: {} });
  });

  it("fails open to the form when the profile cannot be read", async () => {
    const client = queryClient();
    const apiClient = unauthorizedClient();
    seedMe(client, apiClient, false);
    await expect(load(client, apiClient)).resolves.toEqual({ ok: {} });
  });
});

describe("SetPasswordScreen", () => {
  it("renders the card through the route component", () => {
    const component = Route.options.component;
    if (!component) throw new Error("missing component");
    render(React.createElement(component as () => React.ReactElement));
    expect(screen.getByTestId("set-card")).toBeInTheDocument();
  });

  // Last: leaves a toast behind in the (per-file) manager on purpose.
  it("mounts the shared toast host", async () => {
    render(<SetPasswordScreen />);
    act(() => {
      showToast(getToastManager(), { title: "Host is live" });
    });
    expect(await screen.findByText("Host is live")).toBeInTheDocument();
  });
});
