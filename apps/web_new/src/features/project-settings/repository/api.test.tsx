// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Repository and estimates data hooks (PSET-049–052): the query keys, the
// single-project read wiring and the failure notices the tabs rely on.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { repositoryKeys, repositoryQueryOptions, useToggleRepositorySync, useUnbindRepository } from "./api.js";

const WORKSPACE = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const REPOSITORY_PATH = `/api/workspaces/${WORKSPACE}/projects/${PROJECT}/repository/`;

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
  notifyInfo: vi.fn(),
}));

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

function binding(): Record<string, unknown> {
  return {
    bound: true,
    id: "66666666-6666-4666-8666-666666666666",
    provider: "github",
    provider_account_id: null,
    host_url: "https://github.com",
    repository: {
      id: "repo-1",
      provider: "github",
      host_url: "https://github.com",
      namespace: "parity",
      name: "parity-seed",
      full_name: "parity/parity-seed",
      web_url: "https://github.com/parity/parity-seed",
      clone_url_http: "https://github.com/parity/parity-seed.git",
      clone_url_ssh: "git@github.com:parity/parity-seed.git",
      default_branch: "main",
      private: false,
    },
    is_sync_enabled: true,
    clone_auth_mode: "public",
    last_synced_at: null,
    last_sync_error: "",
    degraded: false,
    degraded_reason: null,
  };
}

function stubApi(method: "PATCH" | "DELETE", status = 200) {
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(REPOSITORY_PATH) && (init.method ?? "GET") === method) {
      if (status !== 200) return jsonResponse(url, status, {});
      return jsonResponse(url, 200, method === "DELETE" ? { bound: false } : { ...binding(), is_sync_enabled: false });
    }
    if (url.endsWith(REPOSITORY_PATH)) return jsonResponse(url, 200, binding());
    throw new Error(`unexpected fetch ${url}`);
  }) as never);
}

interface WrapperProps {
  children?: unknown;
}

function wrap(): ({ children }: WrapperProps) => React.ReactElement {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return ({ children }) => <QueryClientProvider client={client}>{children as never}</QueryClientProvider>;
}

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("repository data hooks", () => {
  it("keys the read under the workspace and project", () => {
    expect(repositoryKeys.project(WORKSPACE, PROJECT).slice(0, 3)).toEqual(["ws", WORKSPACE, "project-repository"]);
    const options = repositoryQueryOptions({} as never, WORKSPACE, PROJECT);
    expect(options.queryKey).toEqual(repositoryKeys.project(WORKSPACE, PROJECT));
    expect(options.staleTime).toBeGreaterThan(0);
    expect(options.refetchOnWindowFocus).toBe(false);
  });

  it("writes the sync flag and caches the answer", async () => {
    stubApi("PATCH");
    const { result } = renderHook(() => useToggleRepositorySync(WORKSPACE, PROJECT), { wrapper: wrap() });
    result.current.mutate(false);
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
    expect(notifySuccess).toHaveBeenCalledWith("Sync setting saved", expect.any(String));
    await waitFor(() => expect(result.current.isPending).toBe(false));
  });

  it("notices a refused sync write", async () => {
    stubApi("PATCH", 500);
    const { result } = renderHook(() => useToggleRepositorySync(WORKSPACE, PROJECT), { wrapper: wrap() });
    result.current.mutate(true);
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn’t change sync", expect.any(String)));
  });

  it("unbinds and notices the mirrored-issues guarantee", async () => {
    stubApi("DELETE");
    const { result } = renderHook(() => useUnbindRepository(WORKSPACE, PROJECT), { wrapper: wrap() });
    result.current.mutate();
    await waitFor(() =>
      expect(notifySuccess).toHaveBeenCalledWith("Repository unlinked", "Mirrored issues remain editable.")
    );
  });

  it("notices a refused unbind", async () => {
    stubApi("DELETE", 500);
    const { result } = renderHook(() => useUnbindRepository(WORKSPACE, PROJECT), { wrapper: wrap() });
    result.current.mutate();
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
  });
});
