// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { createClient, type ApiClient } from "@pidash/api-client";
import { isRedirect } from "@tanstack/react-router";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

// The expired-session hook reads the router; navigation stays a spy.
vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    useRouter: () => ({
      state: { location: { href: "/settings/profile/notifications" } },
      navigate: vi.fn(),
      history: { back: vi.fn() },
    }),
  };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { meQueryOptions, profileQueryOptions } from "../../../core/session/queries.js";
import { EMAIL_PREFS_HEADING } from "../../../features/notifications/index.js";
import { EmailPreferencesScreen, Route } from "./notifications.js";

function throwingClient(): ApiClient {
  return createClient({
    baseUrl: "https://api.test",
    transport: (url) => {
      throw new Error(`unexpected fetch ${url}`);
    },
  });
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
  const beforeLoad = Route.options.beforeLoad;
  if (!beforeLoad) throw new Error("missing beforeLoad");
  return beforeLoad({
    context: { queryClient: client, apiClient },
    location: { href: "/settings/profile/notifications" },
  } as never).then(
    (value: unknown) => ({ ok: value }),
    (error: unknown) => ({ thrown: error })
  );
}

function prefsRow() {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    user: "22222222-2222-4222-8222-222222222222",
    workspace: null,
    project: null,
    property_change: true,
    state_change: true,
    comment: false,
    mention: true,
    issue_completed: false,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-02T00:00:00Z",
    created_by: null,
    updated_by: null,
  };
}

afterEach(() => {
  resetAppClient();
});

describe("email preferences guard", () => {
  it("bounces signed-out visitors to the entry with the return path", async () => {
    const outcome = await load(queryClient(), unauthorizedClient());
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: { options: unknown } }).thrown.options).toMatchObject({
      to: "/",
      search: { next_path: "/settings/profile/notifications" },
    });
  });

  it("sends unfinished members to onboarding", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    seedMe(client, apiClient);
    seedProfile(client, apiClient, { is_onboarded: false, onboarding_step: {} });
    const outcome = await load(client, apiClient);
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: { options: unknown } }).thrown.options).toMatchObject({
      href: "/onboarding",
    });
  });

  it("fails closed to onboarding when the profile cannot be read", async () => {
    const client = queryClient();
    const apiClient = unauthorizedClient();
    seedMe(client, apiClient);
    const outcome = await load(client, apiClient);
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: { options: unknown } }).thrown.options).toMatchObject({
      href: "/onboarding",
    });
  });

  it("renders for finished members", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    seedMe(client, apiClient);
    seedProfile(client, apiClient, { is_onboarded: true });
    await expect(load(client, apiClient)).resolves.toEqual({ ok: {} });
  });
});

describe("EmailPreferencesScreen", () => {
  it("mounts the preferences content with the toast host", async () => {
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () => JSON.stringify({ csrf_token: "csrf" }),
        };
      }
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(prefsRow()),
      };
    }) as never);
    render(
      <QueryClientProvider client={queryClient()}>
        <EmailPreferencesScreen />
      </QueryClientProvider>
    );

    await screen.findByTestId("email-prefs-form");
    expect(screen.getByRole("heading", { name: EMAIL_PREFS_HEADING })).toBeInTheDocument();
    expect(screen.getByTestId("toast-stack")).toBeInTheDocument();
  });
});
