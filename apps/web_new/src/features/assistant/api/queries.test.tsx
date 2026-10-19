// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant reads (NEWFRONT-603): the workspace thread list and the
// per-user model config, plus the start mutation's cache patch.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { AssistantThread } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { assistantKeys } from "./keys.js";
import { useStartConversation } from "./mutations.js";
import { assistantConfigQueryOptions, assistantThreadsQueryOptions, useAssistantThreads } from "./queries.js";

const THREAD: AssistantThread = {
  id: "11111111-1111-4111-8111-111111111111",
  title: "",
  is_archived: false,
  has_active_turn: false,
  created_at: "2026-10-01T00:00:00Z",
  updated_at: "2026-10-02T00:00:00Z",
};

const CONFIG = {
  provider_kind: "openai",
  base_url: "https://api.test",
  model_name: "pilot",
  has_api_key: true,
  last_verified_at: null,
};

describe("assistantThreadsQueryOptions", () => {
  it("keys the thread list under the assistant subtree with list freshness", () => {
    const options = assistantThreadsQueryOptions({} as never, "acme");
    expect(options.queryKey).toEqual(["ws", "acme", "assistant", "threads"]);
    expect(options.staleTime).toBe(30_000);
    expect(options.enabled).toBe(true);
  });

  it("passes a disabled read through for hidden surfaces", () => {
    expect(assistantThreadsQueryOptions({} as never, "acme", false).enabled).toBe(false);
  });

  it("reads threads through the contract", async () => {
    const getJson = vi.fn(async () => [THREAD]);
    const options = assistantThreadsQueryOptions({ getJson, parse: (_s: unknown, d: unknown) => d } as never, "acme");
    await expect(options.queryFn?.({} as never)).resolves.toEqual([THREAD]);
    expect(getJson).toHaveBeenCalledWith("/api/workspaces/acme/ai-assistant/threads/");
  });
});

describe("assistantConfigQueryOptions", () => {
  it("keys the config with reference freshness", () => {
    const options = assistantConfigQueryOptions({} as never, "acme");
    expect(options.queryKey).toEqual(["ws", "acme", "assistant", "config"]);
    expect(options.staleTime).toBe(5 * 60_000);
  });

  it("reads the model config through the contract", async () => {
    const getJson = vi.fn(async () => CONFIG);
    const options = assistantConfigQueryOptions({ getJson, parse: (_s: unknown, d: unknown) => d } as never, "acme");
    await expect(options.queryFn?.({} as never)).resolves.toEqual(CONFIG);
    expect(getJson).toHaveBeenCalledWith("/api/users/me/ai-assistant/config/");
  });
});

function renderStart(queryClient: QueryClient) {
  return renderHook(() => useStartConversation(), {
    wrapper: ({ children }: { children?: unknown }) => (
      <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>
    ),
  });
}

function stubClient(bodies: { threads: unknown[]; created: unknown; status?: number }) {
  return createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    if (url.endsWith("/ai-assistant/threads/") && init.method === "POST") {
      return {
        status: bodies.status ?? 201,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(bodies.created),
      };
    }
    return {
      status: 200,
      url,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(bodies.threads),
    };
  }) as never);
}

afterEach(() => {
  resetAppClient();
});

describe("useStartConversation", () => {
  it("prepends the created conversation to the cached list", async () => {
    stubClient({ threads: [], created: THREAD });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(assistantKeys.threads("acme"), [{ ...THREAD, id: "older" }]);
    const { result } = renderStart(queryClient);
    result.current.mutate({ workspaceSlug: "acme" });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(queryClient.getQueryData(assistantKeys.threads("acme"))).toEqual([THREAD, { ...THREAD, id: "older" }]);
  });

  it("starts from an uncached list", async () => {
    stubClient({ threads: [], created: THREAD });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderStart(queryClient);
    result.current.mutate({ workspaceSlug: "acme" });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(queryClient.getQueryData(assistantKeys.threads("acme"))).toEqual([THREAD]);
  });

  it("surfaces a refusal without touching the cache", async () => {
    stubClient({ threads: [], created: { error: "role_not_allowed" }, status: 403 });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(assistantKeys.threads("acme"), []);
    const { result } = renderStart(queryClient);
    result.current.mutate({ workspaceSlug: "acme" });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(queryClient.getQueryData(assistantKeys.threads("acme"))).toEqual([]);
  });
});

describe("useAssistantThreads", () => {
  it("does not fetch while disabled", async () => {
    const client = stubClient({ threads: [THREAD], created: THREAD });
    const spy = vi.spyOn(client, "getJson");
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    renderHook(() => useAssistantThreads("acme", false), {
      wrapper: ({ children }: { children?: unknown }) => (
        <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>
      ),
    });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(spy).not.toHaveBeenCalled();
  });
});
