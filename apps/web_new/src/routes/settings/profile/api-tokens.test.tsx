// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createClient, type ApiClient } from "@pidash/api-client";
import { isRedirect } from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    useRouter: () => ({
      state: { location: { href: "/settings/profile/api-tokens" } },
      navigate: vi.fn(),
      history: { back: vi.fn() },
    }),
  };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { meQueryOptions, profileQueryOptions } from "../../../core/session/queries.js";
import { PersonalAccessTokensRouteComponent, Route } from "./api-tokens.js";

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

function seedMe(client: QueryClient, apiClient: ApiClient): void {
  client.setQueryData(meQueryOptions(apiClient).queryKey, {
    id: "u1",
    email: "ada@example.com",
    first_name: "Ada",
    last_name: "Lovelace",
    is_password_autoset: false,
  } as never);
}

function seedProfile(client: QueryClient, apiClient: ApiClient, profile: Record<string, unknown>): void {
  client.setQueryData(profileQueryOptions(apiClient).queryKey, profile as never);
}

async function load(client: QueryClient, apiClient: ApiClient): Promise<{ ok: unknown } | { thrown: unknown }> {
  const beforeLoad = Route.options.beforeLoad;
  if (!beforeLoad) throw new Error("missing beforeLoad");
  return beforeLoad({
    context: { queryClient: client, apiClient },
    location: { href: "/settings/profile/api-tokens" },
  } as never).then(
    (value: unknown) => ({ ok: value }),
    (error: unknown) => ({ thrown: error })
  );
}

afterEach(() => {
  resetAppClient();
});

describe("personal access tokens guard", () => {
  it("bounces signed-out visitors to the entry with the return path", async () => {
    const outcome = await load(queryClient(), unauthorizedClient());
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: { options: unknown } }).thrown.options).toMatchObject({
      to: "/",
      search: { next_path: "/settings/profile/api-tokens" },
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

  it("fails closed to onboarding when the profile cannot be read", async () => {
    const client = queryClient();
    const apiClient = unauthorizedClient();
    seedMe(client, apiClient);
    const outcome = await load(client, apiClient);
    expect("thrown" in outcome && isRedirect(outcome.thrown)).toBe(true);
    expect((outcome as { thrown: { options: unknown } }).thrown.options).toMatchObject({ href: "/onboarding" });
  });

  it("renders for finished members", async () => {
    const client = queryClient();
    const apiClient = throwingClient();
    seedMe(client, apiClient);
    seedProfile(client, apiClient, { is_onboarded: true });
    await expect(load(client, apiClient)).resolves.toEqual({ ok: {} });
  });
});

describe("PersonalAccessTokensRouteComponent", () => {
  it("mounts the token page inside the main surface with a toast host", async () => {
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () => JSON.stringify({ csrf_token: "csrf" }),
        };
      }
      return { status: 200, url, headers: { get: () => null }, text: async () => "[]" };
    }) as never);
    render(
      <QueryClientProvider client={queryClient()}>
        <PersonalAccessTokensRouteComponent />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("heading", { name: "Personal Access Tokens" })).toBeInTheDocument();
    expect(screen.getByRole("main")).toBeInTheDocument();
    expect(screen.getByTestId("toast-stack")).toBeInTheDocument();
  });
});
