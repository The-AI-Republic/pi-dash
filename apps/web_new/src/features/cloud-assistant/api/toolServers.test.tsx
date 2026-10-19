// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tool-server writes (CLOUD-036): each hook hits its own endpoint through
// the app client and refreshes the list so the section never shows a
// registry the server has already changed.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  toolServerKeys,
  toolServersQueryOptions,
  useAddToolServer,
  useRemoveToolServer,
  useUpdateToolServer,
} from "./toolServers.js";

const ID = "123e4567-e89b-12d3-a456-426614174000";

const SERVER = {
  id: ID,
  name: "probe",
  url: "https://probe.test/mcp",
  has_auth_header: false,
  tool_prefix: "probe",
  effective_tool_prefix: "probe_*",
  is_enabled: true,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
};

function stubTransport(answer: (method: string, url: string) => { status: number; body?: unknown } | undefined) {
  const calls: string[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    const method = init.method ?? "GET";
    calls.push(`${method} ${url}`);
    const reply = answer(method, url);
    if (reply === undefined) throw new Error(`unexpected call ${method} ${url}`);
    const text = reply.body === undefined ? "" : JSON.stringify(reply.body);
    return {
      status: reply.status,
      url,
      headers: { get: () => (reply.body === undefined ? null : "application/json") },
      text: async () => text,
    };
  }) as never);
  return calls;
}

function setup<T>(hook: () => T) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const wrapper = ({ children }: { children?: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>
  );
  return { ...renderHook(hook, { wrapper }), queryClient };
}

afterEach(() => {
  resetAppClient();
});

describe("tool-server writes", () => {
  it("reads the registry at the shared tool-server key", async () => {
    const getJson = vi.fn(async () => [SERVER]);
    const options = toolServersQueryOptions({ getJson, parse: (_schema: unknown, data: unknown) => data } as never);
    expect(options.queryKey).toEqual(toolServerKeys.list());
    await options.queryFn?.({} as never);
    expect(getJson).toHaveBeenCalledWith("/api/users/me/ai-assistant/mcp-servers/");
  });

  it("adds a server and refreshes the list", async () => {
    const calls = stubTransport((method) => (method === "POST" ? { status: 201, body: SERVER } : undefined));
    const { result, queryClient } = setup(() => useAddToolServer());
    queryClient.setQueryData(toolServerKeys.list(), [SERVER]);
    await act(async () => {
      await result.current.mutateAsync({ name: "probe", url: "https://probe.test/mcp" });
    });
    expect(calls.some((call) => call.startsWith("POST https://api.test/api/users/me/ai-assistant/mcp-servers/"))).toBe(
      true
    );
    expect(queryClient.getQueryState(toolServerKeys.list())?.isInvalidated).toBe(true);
  });

  it("patches one server and refreshes the list", async () => {
    const calls = stubTransport((method) => (method === "PATCH" ? { status: 200, body: SERVER } : undefined));
    const { result, queryClient } = setup(() => useUpdateToolServer());
    queryClient.setQueryData(toolServerKeys.list(), [SERVER]);
    await act(async () => {
      await result.current.mutateAsync({ serverId: ID, patch: { is_enabled: false } });
    });
    expect(
      calls.some((call) => call.startsWith(`PATCH https://api.test/api/users/me/ai-assistant/mcp-servers/${ID}/`))
    ).toBe(true);
    expect(queryClient.getQueryState(toolServerKeys.list())?.isInvalidated).toBe(true);
  });

  it("removes one server and refreshes the list", async () => {
    const calls = stubTransport((method) => (method === "DELETE" ? { status: 204 } : undefined));
    const { result, queryClient } = setup(() => useRemoveToolServer());
    queryClient.setQueryData(toolServerKeys.list(), [SERVER]);
    await act(async () => {
      await result.current.mutateAsync(ID);
    });
    expect(
      calls.some((call) => call.startsWith(`DELETE https://api.test/api/users/me/ai-assistant/mcp-servers/${ID}/`))
    ).toBe(true);
    expect(queryClient.getQueryState(toolServerKeys.list())?.isInvalidated).toBe(true);
  });
});
