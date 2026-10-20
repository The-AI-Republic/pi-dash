// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createClient, type ApiClient, type Transport } from "@pidash/api-client";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  isNotFound,
  notFound,
  RouterProvider,
} from "@tanstack/react-router";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { meQueryOptions, workspacesQueryOptions } from "../../core/session/queries.js";
import type { RouterContext } from "../__root.js";
import { Route } from "./route.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";

function membershipRow(role: number) {
  return {
    id: UUID,
    workspace: UUID,
    member: UUID,
    role,
    company_role: null,
    view_props: {},
    default_props: {},
    issue_props: {},
    is_active: true,
    getting_started_checklist: {},
    tips: {},
    explored_features: {},
    draft_issue_count: 0,
    created_by: null,
    updated_by: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
  };
}

function stubTransport(handler: (url: string) => { status: number; body: string; url: string }): Transport {
  return async (url) => {
    const response = handler(url);
    return {
      status: response.status,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
}

function throwingClient(): ApiClient {
  return createClient({
    baseUrl: "https://api.test",
    transport: (url) => {
      throw new Error(`unexpected fetch ${url}`);
    },
  });
}

function queryClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function seedSession(client: QueryClient, apiClient: ApiClient, workspaces: unknown) {
  client.setQueryData(meQueryOptions(apiClient).queryKey, { id: "u1", email: "ada@example.com" } as never);
  client.setQueryData(workspacesQueryOptions(apiClient).queryKey, workspaces as never);
}

async function load(
  client: QueryClient,
  apiClient: ApiClient,
  ws: string
): Promise<{ ok: unknown } | { thrown: unknown }> {
  const beforeLoad = Route.options.beforeLoad;
  if (!beforeLoad) throw new Error("missing beforeLoad");
  return beforeLoad({ context: { queryClient: client, apiClient }, params: { ws } } as never).then(
    (value: unknown) => ({ ok: value }),
    (error: unknown) => ({ thrown: error })
  );
}

describe("workspace gate", () => {
  it("passes signed-out visitors through", async () => {
    const apiClient = createClient({
      baseUrl: "https://api.test",
      transport: stubTransport((url) => ({ status: 401, url, body: JSON.stringify({ detail: "signed out" }) })),
    });
    await expect(load(queryClient(), apiClient, "acme")).resolves.toEqual({ ok: {} });
  });

  it("denies signed-in visitors on unknown slugs", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    seedSession(client, apiClient, [{ id: "w1", slug: "acme", role: 15 }]);
    const outcome = await load(client, apiClient, "foreign");
    expect("thrown" in outcome && isNotFound(outcome.thrown)).toBe(true);
  });

  it("denies signed-in visitors when the workspace list cannot be read", async () => {
    const client = queryClient();
    const apiClient = createClient({
      baseUrl: "https://api.test",
      transport: stubTransport((url) => ({ status: 500, url, body: "{}" })),
    });
    // The seeded me row wins; the workspace list fetch fails.
    client.setQueryData(meQueryOptions(apiClient).queryKey, { id: "u1" } as never);
    const outcome = await load(client, apiClient, "acme");
    expect("thrown" in outcome && isNotFound(outcome.thrown)).toBe(true);
  });

  it("renders members with their membership info", async () => {
    const client = queryClient();
    const apiClient = createClient({
      baseUrl: "https://api.test",
      transport: stubTransport((url) => {
        if (url.endsWith("/workspace-members/me/")) {
          return { status: 200, url, body: JSON.stringify(membershipRow(15)) };
        }
        if (url.endsWith("/project-roles/")) {
          return { status: 200, url, body: "{}" };
        }
        throw new Error(`unexpected fetch ${url}`);
      }),
    });
    // Seeded rows win for me and the list; the feeds fetch.
    seedSession(client, apiClient, [{ id: "w1", slug: "acme", role: 15 }]);
    await expect(load(client, apiClient, "acme")).resolves.toEqual({ ok: {} });
  });

  it("denies known slugs without membership info", async () => {
    const client = queryClient();
    const apiClient = createClient({
      baseUrl: "https://api.test",
      transport: stubTransport((url) => {
        if (url.endsWith("/workspace-members/me/")) return { status: 200, url, body: "null" };
        if (url.endsWith("/project-roles/")) return { status: 200, url, body: "{}" };
        throw new Error(`unexpected fetch ${url}`);
      }),
    });
    seedSession(client, apiClient, [{ id: "w1", slug: "acme", role: 15 }]);
    const outcome = await load(client, apiClient, "acme");
    expect("thrown" in outcome && isNotFound(outcome.thrown)).toBe(true);
  });

  it("denies like a missing membership when the read fails", async () => {
    const client = queryClient();
    const apiClient = createClient({
      baseUrl: "https://api.test",
      transport: stubTransport((url) => ({ status: 500, url, body: "{}" })),
    });
    seedSession(client, apiClient, [{ id: "w1", slug: "acme", role: 15 }]);
    const outcome = await load(client, apiClient, "acme");
    expect("thrown" in outcome && isNotFound(outcome.thrown)).toBe(true);
  });

  it("tolerates a failed roles warm for members", async () => {
    const client = queryClient();
    const apiClient = createClient({
      baseUrl: "https://api.test",
      transport: stubTransport((url) => {
        if (url.endsWith("/workspace-members/me/")) {
          return { status: 200, url, body: JSON.stringify(membershipRow(15)) };
        }
        return { status: 500, url, body: "{}" };
      }),
    });
    seedSession(client, apiClient, [{ id: "w1", slug: "acme", role: 15 }]);
    await expect(load(client, apiClient, "acme")).resolves.toEqual({ ok: {} });
  });
});

describe("WorkspaceDenialNotice", () => {
  function denialRouter(entry: string, apiClient: ApiClient, client: QueryClient) {
    const notFoundComponent = Route.options.notFoundComponent;
    if (!notFoundComponent) throw new Error("missing notFoundComponent");
    const rootRoute = createRootRouteWithContext<RouterContext>()();
    const homeRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/",
      component: () => null,
    });
    const wsRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/$ws",
      beforeLoad: () => {
        throw notFound();
      },
      notFoundComponent,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([homeRoute, wsRoute]),
      context: { queryClient: client, apiClient },
      history: createMemoryHistory({ initialEntries: [entry] }),
    });
    return router;
  }

  function renderDenial(entry: string, workspaces: unknown, transport: Transport) {
    const apiClient = createAppClient("https://api.test", transport);
    const client = queryClient();
    client.setQueryData(meQueryOptions(apiClient).queryKey, { id: "u1", email: "ada@example.com" } as never);
    if (workspaces !== undefined) {
      client.setQueryData(workspacesQueryOptions(apiClient).queryKey, workspaces as never);
    }
    const router = denialRouter(entry, apiClient, client);
    render(
      <QueryClientProvider client={client}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    );
    return router;
  }

  afterEach(() => {
    resetAppClient();
  });

  it("reports not-found with a way home for unknown slugs", async () => {
    const transport = stubTransport((url) => {
      throw new Error(`unexpected fetch ${url}`);
    });
    renderDenial("/foreign", [{ id: "w1", slug: "acme", role: 15 }], transport);
    expect(await screen.findByRole("heading", { name: "Workspace not found" })).toBeVisible();
    expect(screen.getByText("ada@example.com")).toBeVisible();
    const home = screen.getByRole("link", { name: "Go Home" });
    expect(home).toHaveFocus();
    expect(screen.getByRole("link", { name: "Open profile" })).toHaveAttribute("href", "/settings/profile/general/");
  });

  it("offers creation when the user belongs nowhere", async () => {
    const transport = stubTransport((url) => {
      throw new Error(`unexpected fetch ${url}`);
    });
    renderDenial("/foreign", [], transport);
    const create = await screen.findByRole("link", { name: "Create a workspace" });
    expect(create).toHaveFocus();
    expect(screen.queryByRole("link", { name: "Go Home" })).not.toBeInTheDocument();
  });

  it("renders the not-a-member screen for known slugs", async () => {
    const transport = stubTransport((url) => {
      throw new Error(`unexpected fetch ${url}`);
    });
    renderDenial("/acme", [{ id: "w1", slug: "acme", role: 15 }], transport);
    const invites = await screen.findByRole("link", { name: "View pending invitations" });
    expect(invites).toHaveFocus();
    expect(screen.getByRole("heading", { name: "Not Authorized!" })).toBeVisible();
  });

  it("renders nothing while the session reads load", async () => {
    const transport: Transport = async (url) => {
      await new Promise<void>(() => undefined);
      return { status: 200, url, headers: { get: () => null }, text: async () => "{}" };
    };
    const apiClient = createAppClient("https://api.test", transport);
    const client = queryClient();
    const router = denialRouter("/foreign", apiClient, client);
    render(
      <QueryClientProvider client={client}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    );
    // The reads never settle, so nothing may render, however long we wait.
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 100));
    });
    expect(screen.queryByRole("heading")).not.toBeInTheDocument();
  });

  it("fails open to creation when the reads error", async () => {
    const transport = stubTransport((url) => ({ status: 500, url, body: "{}" }));
    const apiClient = createAppClient("https://api.test", transport);
    const client = queryClient();
    const router = denialRouter("/foreign", apiClient, client);
    render(
      <QueryClientProvider client={client}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("heading", { name: "Workspace not found" })).toBeVisible();
    expect(screen.queryByText("ada@example.com")).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Create a workspace" })).toBeVisible();
  });

  it("signs out from the header and lands on the entry", async () => {
    const posts: string[] = [];
    const transport = stubTransport((url) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return { status: 200, url, body: JSON.stringify({ csrf_token: "csrf" }) };
      }
      if (url.endsWith("/auth/sign-out/")) {
        posts.push(url);
        return { status: 200, url, body: "{}" };
      }
      throw new Error(`unexpected fetch ${url}`);
    });
    const router = renderDenial("/foreign", [{ id: "w1", slug: "acme", role: 15 }], transport);
    await screen.findByRole("heading", { name: "Workspace not found" });
    await userEvent.setup().click(screen.getByRole("button", { name: "Sign out" }));
    await waitFor(() => expect(posts).toHaveLength(1));
    await waitFor(() => expect(router.state.location.pathname).toBe("/"));
  });
});
