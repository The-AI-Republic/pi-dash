// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { createClient, type ApiClient, type Transport } from "@pidash/api-client";
import { isRedirect } from "@tanstack/react-router";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// The real router except navigation: the guard throws real redirects while
// the expired-redirect hook reads a stubbed location.
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useRouter: () => ({ state: { location: { href: "/onboarding" } }, navigate: vi.fn() }),
}));

// Stand-in for the flow: its internals belong to the auth area. What this
// suite owns is the funnel guard plus the screen wiring. The stand-in
// records the props it receives so the suite can assert which steps the
// screen mounts inside the flow.
const seenFlowProps = vi.hoisted(() => ({ props: [] as unknown[] }));
vi.mock("../features/auth/components/OnboardingFlow.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../features/auth/components/OnboardingFlow.js")>();
  return {
    ...original,
    OnboardingFlow: (props: unknown) => {
      seenFlowProps.props.push(props);
      return <div data-testid="onboarding-flow" />;
    },
  };
});

import {
  meQueryOptions,
  profileQueryOptions,
  settingsQueryOptions,
  workspacesQueryOptions,
} from "../core/session/queries.js";
import { InviteMembersStep } from "../features/auth/components/InviteMembersStep.js";
import { WorkspaceStep } from "../features/auth/components/WorkspaceStep.js";
import { myInvitationsQueryOptions } from "../features/auth/invitations.js";
import { Route } from "./onboarding.js";

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

function seedFlowReads(client: QueryClient, apiClient: ApiClient) {
  client.setQueryData(workspacesQueryOptions(apiClient).queryKey, []);
  client.setQueryData(myInvitationsQueryOptions(apiClient).queryKey, []);
}

async function load(client: QueryClient, apiClient: ApiClient): Promise<{ ok: unknown } | { thrown: unknown }> {
  return beforeLoadOf(Route)({
    context: { queryClient: client, apiClient },
    location: { href: "/onboarding" },
  } as never).then(
    (value: unknown) => ({ ok: value }),
    (error: unknown) => ({ thrown: error })
  );
}

describe("onboarding funnel guard", () => {
  it("bounces signed-out visitors to the entry with the return path", async () => {
    const outcome = await load(queryClient(), unauthorizedClient());
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: { options: unknown } }).thrown.options).toMatchObject({
      to: "/",
      search: { next_path: "/onboarding" },
    });
  });

  it("renders the flow for unfinished users", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    seedMe(client, apiClient, false);
    seedProfile(client, apiClient, { is_onboarded: false });
    seedFlowReads(client, apiClient);
    await expect(load(client, apiClient)).resolves.toEqual({ ok: {} });
  });

  it("sends finished users to the landing route", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    seedMe(client, apiClient, false);
    seedProfile(client, apiClient, { is_onboarded: true });
    client.setQueryData(settingsQueryOptions(apiClient).queryKey, {
      workspace: { last_workspace_slug: "acme", fallback_workspace_slug: null },
    } as never);
    client.setQueryData(workspacesQueryOptions(apiClient).queryKey, [{ id: "w1", slug: "acme" }] as never);
    const outcome = await load(client, apiClient);
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: { options: unknown } }).thrown.options).toMatchObject({ href: "/acme" });
  });

  it("sends all-flags profiles without the marker to the landing route", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    seedMe(client, apiClient, false);
    seedProfile(client, apiClient, { is_onboarded: false, onboarding_step: { ...ALL_STEP_FLAGS } });
    client.setQueryData(settingsQueryOptions(apiClient).queryKey, {
      workspace: { last_workspace_slug: "acme", fallback_workspace_slug: null },
    } as never);
    client.setQueryData(workspacesQueryOptions(apiClient).queryKey, [{ id: "w1", slug: "acme" }] as never);
    const outcome = await load(client, apiClient);
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: { options: unknown } }).thrown.options).toMatchObject({ href: "/acme" });
  });

  it("renders when the prefetch reads fail", async () => {
    const client = queryClient();
    const apiClient = unauthorizedClient();
    seedMe(client, apiClient, false);
    seedProfile(client, apiClient, { is_onboarded: false });
    await expect(load(client, apiClient)).resolves.toEqual({ ok: {} });
  });

  it("fails closed to the flow when the profile cannot be read", async () => {
    const client = queryClient();
    const apiClient = unauthorizedClient();
    seedMe(client, apiClient, false);
    await expect(load(client, apiClient)).resolves.toEqual({ ok: {} });
  });
});

describe("OnboardingScreen", () => {
  it("renders the flow through the route component", () => {
    const component = Route.options.component;
    if (!component) throw new Error("missing component");
    render(React.createElement(component as () => React.ReactElement));
    expect(screen.getByTestId("onboarding-flow")).toBeInTheDocument();
  });

  it("mounts the workspace and invite steps inside the flow", () => {
    seenFlowProps.props.length = 0;
    const component = Route.options.component;
    if (!component) throw new Error("missing component");
    render(React.createElement(component as () => React.ReactElement));
    expect(seenFlowProps.props).toHaveLength(1);
    const steps = (seenFlowProps.props[0] as { steps: Record<string, unknown> }).steps;
    expect(steps["workspace"]).toBe(WorkspaceStep);
    expect(steps["invite"]).toBe(InviteMembersStep);
  });
});
