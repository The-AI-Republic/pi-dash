// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Integrations hooks (parity/specs/profile.md §11, PROF-025): the status
// read, the install handoff address, and a refresh that replaces only the
// workspace it checked — each failure reporting the server's own message.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { Transport, TransportInit } from "@pidash/api-client";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { notifySuccess, notifyError } = vi.hoisted(() => ({
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

vi.mock("../../shared/shell/toaster.js", () => ({ notifySuccess, notifyError }));

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import {
  integrationsKeys,
  useGithubAppStatus,
  useRefreshGithubInstall,
  useStartGithubInstall,
  withRefreshedInstallation,
} from "./api.js";

const ID_ONE = "3f7c1d1e-0d1a-4c59-9f0b-6a2f3e4d5c6b";
const ID_TWO = "8a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

const STATUS = {
  configured: true,
  app_slug: "pi-dash",
  workspaces: [
    { id: ID_ONE, slug: "acme", name: "Acme", github_app: { connected: true, account_login: "octo" } },
    { id: ID_TWO, slug: "beta", name: "Beta", github_app: { connected: false } },
  ],
};

type Reply = { status: number; body?: unknown };

function stubTransport(handler: () => Reply) {
  const calls: { method: string; url: string; body: unknown }[] = [];
  const transport: Transport = async (url: string, init: TransportInit) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ csrf_token: "t" }) };
    }
    calls.push({
      method: init.method,
      url,
      body: typeof init.body === "string" ? (JSON.parse(init.body) as unknown) : init.body,
    });
    const reply = handler();
    return {
      status: reply.status,
      url,
      headers: { get: () => null },
      text: async () => (reply.body === undefined ? "" : JSON.stringify(reply.body)),
    };
  };
  return { transport, calls };
}

function setup(handler: () => Reply) {
  const { transport, calls } = stubTransport(handler);
  createAppClient("https://api.test", transport);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={client}>{children as never}</QueryClientProvider>;
  }
  return { calls, client, Wrapper };
}

beforeEach(() => {
  notifySuccess.mockReset();
  notifyError.mockReset();
});
afterEach(() => resetAppClient());

describe("status read", () => {
  it("reads the member's admin-scoped status", async () => {
    const { Wrapper, calls } = setup(() => ({ status: 200, body: STATUS }));
    const { result } = renderHook(() => useGithubAppStatus(), { wrapper: Wrapper });
    await waitFor(() => expect(result.current.data).toEqual(STATUS));
    expect(calls[0]).toMatchObject({ method: "GET", url: "https://api.test/api/users/me/integrations/github/app/" });
  });
});

describe("install handoff", () => {
  it("posts the workspace and answers the host address", async () => {
    const { Wrapper, calls } = setup(() => ({
      status: 200,
      body: { state: "s", expires_at: "2026-10-19T01:00:00Z", install_url: "https://github.test/install" },
    }));
    const { result } = renderHook(() => useStartGithubInstall(), { wrapper: Wrapper });
    act(() => {
      result.current.mutate("acme");
    });
    await waitFor(() => expect(result.current.data).toBe("https://github.test/install"));
    expect(calls[0]).toMatchObject({
      method: "POST",
      url: "https://api.test/api/users/me/integrations/github/app/install/",
      body: { workspace_slug: "acme" },
    });
  });

  it("reports a refused start with the server's error code", async () => {
    const { Wrapper } = setup(() => ({ status: 409, body: { error: "github_app_not_configured" } }));
    const { result } = renderHook(() => useStartGithubInstall(), { wrapper: Wrapper });
    act(() => {
      result.current.mutate("acme");
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith("Install failed", "github_app_not_configured");
  });

  it("prefers the server code over its detail, and falls back with neither", async () => {
    const detailed = setup(() => ({ status: 403, body: { error: "forbidden", detail: "Admins only." } }));
    const first = renderHook(() => useStartGithubInstall(), { wrapper: detailed.Wrapper });
    act(() => {
      first.result.current.mutate("acme");
    });
    await waitFor(() => expect(first.result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenLastCalledWith("Install failed", "forbidden");

    expect(detailed.calls[0]).toMatchObject({ body: { workspace_slug: "acme" } });

    resetAppClient();
    const empty = setup(() => ({ status: 500, body: {} }));
    const second = renderHook(() => useStartGithubInstall(), { wrapper: empty.Wrapper });
    act(() => {
      second.result.current.mutate("acme");
    });
    await waitFor(() => expect(second.result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenLastCalledWith("Install failed", "Could not start the GitHub App install flow.");
  });
});

describe("withRefreshedInstallation", () => {
  it("replaces only the named workspace", () => {
    const next = withRefreshedInstallation(STATUS, "beta", { connected: true, account_login: "beta-bot" });
    expect(next?.workspaces[1]?.github_app).toEqual({ connected: true, account_login: "beta-bot" });
    expect(next?.workspaces[0]).toBe(STATUS.workspaces[0]);
  });

  it("leaves an absent payload absent", () => {
    expect(withRefreshedInstallation(undefined, "beta", { connected: true })).toBeUndefined();
  });
});

describe("refresh", () => {
  it("posts the slug, replaces the cached row and notices", async () => {
    const refreshed = { connected: true, account_login: "octo", last_checked_at: "2026-10-19T12:00:00Z" };
    const { Wrapper, calls, client } = setup(() => ({ status: 200, body: refreshed }));
    client.setQueryData(integrationsKeys.status, STATUS);
    const { result } = renderHook(() => useRefreshGithubInstall(), { wrapper: Wrapper });
    act(() => {
      result.current.mutate("acme");
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls[0]).toMatchObject({
      method: "POST",
      url: "https://api.test/api/users/me/integrations/github/app/refresh/",
      body: { workspace_slug: "acme" },
    });
    expect(client.getQueryData(integrationsKeys.status)).toEqual({
      ...STATUS,
      workspaces: [{ ...STATUS.workspaces[0], github_app: refreshed }, STATUS.workspaces[1]],
    });
    expect(notifySuccess).toHaveBeenCalledWith(
      "GitHub refreshed",
      "The GitHub App installation was checked successfully."
    );
  });

  it("reports a failed re-check without touching the cache", async () => {
    const { Wrapper, client } = setup(() => ({ status: 502, body: { error: "github_verification_failed" } }));
    client.setQueryData(integrationsKeys.status, STATUS);
    const { result } = renderHook(() => useRefreshGithubInstall(), { wrapper: Wrapper });
    act(() => {
      result.current.mutate("acme");
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith("Refresh failed", "github_verification_failed");
    expect(client.getQueryData(integrationsKeys.status)).toEqual(STATUS);
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("falls back when the refusal carries no message", async () => {
    const { Wrapper } = setup(() => ({ status: 500 }));
    const { result } = renderHook(() => useRefreshGithubInstall(), { wrapper: Wrapper });
    act(() => {
      result.current.mutate("acme");
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith("Refresh failed", "Could not check the GitHub App installation.");
  });
});
