// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createClient, type ApiClient, type Transport, type TransportInit } from "@pidash/api-client";
import { isRedirect } from "@tanstack/react-router";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const routerSpies = vi.hoisted(() => ({ navigate: vi.fn(), historyBack: vi.fn() }));

// The real router except navigation: the guard throws real redirects, the
// brand mark renders a plain anchor, and navigation is captured.
vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: ({ to, children, ...rest }: { to: string; children?: React.ReactNode }) => (
      <a href={to} {...rest}>
        {children}
      </a>
    ),
    useNavigate: () => routerSpies.navigate,
    useRouter: () => ({
      state: { location: { href: "/create-workspace" } },
      navigate: routerSpies.navigate,
      history: { back: routerSpies.historyBack },
    }),
  };
});

vi.mock("../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient } from "../core/api/client.js";
import {
  instanceConfigQueryOptions,
  meQueryOptions,
  profileQueryOptions,
  setSessionClient,
} from "../core/session/queries.js";
import { CREATION_DISABLED_ASK_LABEL, CREATION_DISABLED_BACK_LABEL } from "../features/auth/index.js";
import { CreateWorkspaceScreen, Route } from "./create-workspace.js";

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

function seedMe(client: QueryClient, apiClient: ApiClient) {
  client.setQueryData(meQueryOptions(apiClient).queryKey, {
    id: "u1",
    email: "ada@example.com",
    first_name: "Ada",
    last_name: "Lovelace",
    is_password_autoset: false,
  } as never);
}

function seedProfile(client: QueryClient, apiClient: ApiClient, profile: Record<string, unknown>) {
  client.setQueryData(profileQueryOptions(apiClient).queryKey, profile as never);
}

async function load(client: QueryClient, apiClient: ApiClient): Promise<{ ok: unknown } | { thrown: unknown }> {
  return beforeLoadOf(Route)({
    context: { queryClient: client, apiClient },
    location: { href: "/create-workspace" },
  } as never).then(
    (value: unknown) => ({ ok: value }),
    (error: unknown) => ({ thrown: error })
  );
}

describe("create-workspace guard", () => {
  it("bounces signed-out visitors to the entry with the return path", async () => {
    const outcome = await load(queryClient(), unauthorizedClient());
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: { options: unknown } }).thrown.options).toMatchObject({
      to: "/",
      search: { next_path: "/create-workspace" },
    });
  });

  it("sends unfinished members to onboarding", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    seedMe(client, apiClient);
    seedProfile(client, apiClient, { is_onboarded: false, onboarding_step: {} });
    const outcome = await load(client, apiClient);
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: { options: unknown } }).thrown.options).toMatchObject({ href: "/onboarding" });
  });

  it("renders for finished members", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    seedMe(client, apiClient);
    seedProfile(client, apiClient, { is_onboarded: true });
    client.setQueryData(instanceConfigQueryOptions(apiClient).queryKey, {
      is_workspace_creation_disabled: false,
    } as never);
    await expect(load(client, apiClient)).resolves.toEqual({ ok: {} });
  });

  it("renders finished members through the all-flags marker", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    seedMe(client, apiClient);
    seedProfile(client, apiClient, {
      is_onboarded: false,
      onboarding_step: { profile_complete: true, workspace_create: true, workspace_invite: true, workspace_join: true },
    });
    client.setQueryData(instanceConfigQueryOptions(apiClient).queryKey, {
      is_workspace_creation_disabled: false,
    } as never);
    await expect(load(client, apiClient)).resolves.toEqual({ ok: {} });
  });

  it("renders when the config prefetch fails", async () => {
    const client = queryClient();
    const apiClient = unauthorizedClient();
    seedMe(client, apiClient);
    seedProfile(client, apiClient, { is_onboarded: true });
    await expect(load(client, apiClient)).resolves.toEqual({ ok: {} });
  });

  it("fails closed to onboarding when the profile cannot be read", async () => {
    const client = queryClient();
    const apiClient = unauthorizedClient();
    seedMe(client, apiClient);
    const outcome = await load(client, apiClient);
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: { options: unknown } }).thrown.options).toMatchObject({ href: "/onboarding" });
  });
});

const CSRF = JSON.stringify({ csrf_token: "csrf" });

const CREATED_BODY = {
  id: "333e4567-e89b-12d3-a456-426614174002",
  name: "Northwind",
  slug: "northwind",
  logo: null,
  logo_asset: null,
  logo_url: null,
  owner: "123e4567-e89b-12d3-a456-426614174000",
  organization_size: "2-10",
  timezone: "UTC",
  background_color: "",
  total_members: 1,
  role: 20,
  created_by: null,
  updated_by: null,
  created_at: "2024-01-02T03:04:05Z",
  updated_at: "2024-01-02T03:04:05Z",
};

const PROFILE_BODY = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  user: "123e4567-e89b-12d3-a456-426614174000",
  theme: {},
  is_app_rail_docked: false,
  is_tour_completed: false,
  onboarding_step: {},
  use_case: null,
  role: null,
  is_onboarded: true,
  last_workspace_id: null,
  billing_address_country: "",
  billing_address: null,
  has_billing_address: false,
  company_name: "",
  notification_view_mode: "full",
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
  created_at: "2024-01-02T03:04:05Z",
  updated_at: "2024-02-03T04:05:06Z",
};

function configSeed(disabled: boolean) {
  return {
    enable_signup: true,
    is_workspace_creation_disabled: disabled,
    is_google_enabled: false,
    is_github_enabled: false,
    is_gitlab_enabled: false,
    is_gitea_enabled: false,
    is_magic_login_enabled: false,
    is_email_password_enabled: true,
    is_smtp_configured: true,
    is_self_managed: false,
    instance_changelog_url: "",
  };
}

const ME_SEED = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  email: "ada@example.com",
  first_name: "Ada",
  last_name: "Lovelace",
};

function renderScreen(options: {
  me?: typeof ME_SEED | "pending" | "error";
  config?: boolean | "pending" | "error";
}): QueryClient {
  createAppClient("https://api.test", async (url, init: TransportInit) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => CSRF };
    }
    if (url.includes("/api/workspace-slug-check/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ status: true }) };
    }
    if (url.endsWith("/api/workspaces/") && init.method === "POST") {
      return { status: 201, url, headers: { get: () => null }, text: async () => JSON.stringify(CREATED_BODY) };
    }
    if (url.endsWith("/api/users/me/profile/") && init.method === "PATCH") {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(PROFILE_BODY) };
    }
    if (url.endsWith("/api/users/me/workspaces/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify([]) };
    }
    if (url.endsWith("/api/users/me/settings/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () =>
          JSON.stringify({
            id: ME_SEED.id,
            email: ME_SEED.email,
            workspace: {
              last_workspace_id: null,
              last_workspace_slug: null,
              fallback_workspace_id: null,
              fallback_workspace_slug: null,
              invites: 0,
            },
          }),
      };
    }
    return { status: 200, url, headers: { get: () => null }, text: async () => "{}" };
  });
  const failing = options.me === "error" || options.config === "error";
  setSessionClient({
    getJson: failing
      ? async () => {
          throw new Error("offline");
        }
      : () => new Promise<never>(() => {}),
    parse: (_schema: unknown, data: unknown) => data,
  } as never);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (options.me !== undefined && options.me !== "pending" && options.me !== "error") {
    client.setQueryData(["session", "me"], options.me);
  }
  if (options.config !== undefined && options.config !== "pending" && options.config !== "error") {
    client.setQueryData(["instance", "config"], configSeed(options.config));
  }
  render(
    <QueryClientProvider client={client}>
      <CreateWorkspaceScreen />
    </QueryClientProvider>
  );
  return client;
}

afterEach(() => {
  resetAppClient();
  routerSpies.navigate.mockClear();
  routerSpies.historyBack.mockClear();
});

describe("CreateWorkspaceScreen", () => {
  it("renders through the route component", () => {
    const component = Route.options.component;
    if (!component) throw new Error("missing component");
    const RouteComponent = component as unknown as React.ComponentType;
    setSessionClient({ getJson: () => new Promise<never>(() => {}), parse: (_s: unknown, d: unknown) => d } as never);
    render(
      <QueryClientProvider client={queryClient()}>
        <RouteComponent />
      </QueryClientProvider>
    );
    expect(screen.getByText("Loading workspace creation")).toBeInTheDocument();
  });

  it("shows a loader while the account loads", () => {
    renderScreen({ me: "pending", config: false });
    expect(screen.getByText("Loading workspace creation")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Create your workspace" })).not.toBeInTheDocument();
  });

  it("shows a loader while the instance config loads", () => {
    renderScreen({ me: ME_SEED, config: "pending" });
    expect(screen.getByText("Loading workspace creation")).toBeInTheDocument();
  });

  it("shows a loader when the account read fails", async () => {
    renderScreen({ me: "error", config: false });
    expect(await screen.findByText("Loading workspace creation")).toBeInTheDocument();
  });

  it("mounts the shared form with chrome and enters the workspace on success", async () => {
    const user = userEvent.setup();
    renderScreen({ me: ME_SEED, config: false });

    expect(screen.getByRole("link", { name: "Pi Dash" })).toHaveAttribute("href", "/");
    expect(screen.getByText("ada@example.com")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Create your workspace" })).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Workspace name"), { target: { value: "Northwind" } });
    fireEvent.change(screen.getByLabelText("Workspace URL"), { target: { value: "northwind" } });
    await user.selectOptions(screen.getByRole("combobox", { name: "Team size" }), "2-10");
    await user.click(screen.getByRole("button", { name: "Create workspace" }));

    await waitFor(() => expect(routerSpies.navigate).toHaveBeenCalledWith({ to: "/$ws", params: { ws: "northwind" } }));
  });

  it("shows the form when the config read fails", async () => {
    renderScreen({ me: ME_SEED, config: "error" });
    expect(await screen.findByRole("heading", { name: "Create your workspace" })).toBeInTheDocument();
  });

  it("shows the admin-only screen with working actions when creation is disabled", async () => {
    const user = userEvent.setup();
    renderScreen({ me: ME_SEED, config: true });

    expect(
      await screen.findByRole("heading", { name: "Only the instance admin can create workspaces" })
    ).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Create your workspace" })).not.toBeInTheDocument();
    const ask = screen.getByRole("link", { name: CREATION_DISABLED_ASK_LABEL });
    expect(ask.getAttribute("href") ?? "").toContain(encodeURIComponent("ada@example.com"));

    await user.click(screen.getByRole("button", { name: CREATION_DISABLED_BACK_LABEL }));
    expect(routerSpies.historyBack).toHaveBeenCalledTimes(1);
  });
});
