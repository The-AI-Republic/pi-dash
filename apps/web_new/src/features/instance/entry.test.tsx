// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { Transport } from "@pidash/api-client";
import { QueryClient } from "@tanstack/react-query";
import { createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet } from "@tanstack/react-router";
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { createMemoryStore } from "../../core/platform/storage.js";
import { instanceAppKeys } from "./queries.js";
import { AppRoot, appCacheShouldPersist, createAppCachePersistor } from "./entry.js";

function response(url: string, body: unknown): Awaited<ReturnType<Transport>> {
  return {
    status: 200,
    url,
    headers: { get: () => null },
    text: () => Promise.resolve(JSON.stringify(body)),
  };
}

function readyEnvelope(): unknown {
  return {
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
      is_self_managed: true,
      instance_changelog_url: "",
    },
    instance: {
      id: "11111111-1111-4111-8111-111111111111",
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

function testRouter() {
  const rootRoute = createRootRoute({ component: Outlet });
  const indexRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/",
    component: () => <p>route content</p>,
  });
  return createRouter({
    routeTree: rootRoute.addChildren([indexRoute]),
    context: { queryClient: new QueryClient() },
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
}

afterEach(() => {
  resetAppClient();
});

describe("appCacheShouldPersist", () => {
  it("persists only reference keys on web", () => {
    const shouldPersist = appCacheShouldPersist("web");
    expect(shouldPersist(["ws", "acme", "reference", "states"])).toBe(true);
    expect(shouldPersist(instanceAppKeys.bootstrap)).toBe(false);
    expect(shouldPersist(["ws", "acme", "issues"])).toBe(false);
  });

  it("persists everything on desktop except the one-shot startup answer", () => {
    const shouldPersist = appCacheShouldPersist("desktop");
    expect(shouldPersist(["ws", "acme", "issues"])).toBe(true);
    expect(shouldPersist(instanceAppKeys.bootstrap)).toBe(false);
  });
});

describe("createAppCachePersistor", () => {
  it("installs the target's filter and restores the stored payload", async () => {
    const store = createMemoryStore();
    const writer = createQueryClientForTest();
    const persistor = createAppCachePersistor("desktop", writer, store);
    writer.setQueryData(["ws", "acme", "issues"], [{ id: "1" }]);
    writer.setQueryData(instanceAppKeys.bootstrap, { setupDone: true });
    await persistor.save();
    const stored = JSON.parse((await store.get("tanstack-query-cache")) ?? "{}") as {
      state: { queries: { queryKey: unknown[] }[] };
    };
    expect(stored.state.queries.map((query) => query.queryKey)).toEqual([["ws", "acme", "issues"]]);

    const reader = createQueryClientForTest();
    const restorer = createAppCachePersistor("desktop", reader, store);
    await restorer.restore();
    expect(reader.getQueryData(["ws", "acme", "issues"])).toEqual([{ id: "1" }]);
    expect(reader.getQueryData(instanceAppKeys.bootstrap)).toBeUndefined();
  });
});

function createQueryClientForTest(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

describe("AppRoot", () => {
  it("mounts the instance gate above the router and lets a running instance through", async () => {
    createAppClient("https://api.test", () =>
      Promise.resolve(response("https://api.test/api/instances/", readyEnvelope()))
    );
    render(<AppRoot queryClient={createQueryClientForTest()} router={testRouter()} />);
    expect(await screen.findByText("route content")).toBeInTheDocument();
  });

  it("replaces the router with maintenance when the startup read fails", async () => {
    createAppClient("https://api.test", () =>
      Promise.resolve({
        status: 500,
        url: "https://api.test/api/instances/",
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ detail: "down" })),
      })
    );
    render(<AppRoot queryClient={createQueryClientForTest()} router={testRouter()} />);
    await waitFor(() => expect(screen.getByTestId("instance-maintenance")).toBeInTheDocument());
    expect(screen.queryByText("route content")).not.toBeInTheDocument();
  });
});
