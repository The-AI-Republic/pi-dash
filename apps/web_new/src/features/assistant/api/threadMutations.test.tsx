// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Send and stop writes (AGT-042/AGT-043): sends post the content and
// refresh the thread list; stops post cancel; failures read as the
// assistant detail, then its code, then a generic sentence.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "@pidash/api-client";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { assistantKeys } from "./keys.js";
import { threadKey } from "./threadKeys.js";
import { SEND_FAILED_FALLBACK, sendFailureLine, useSendMessage, useStopTurn } from "./threadMutations.js";

function wrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>
        <React.Fragment>{children}</React.Fragment>
      </QueryClientProvider>
    );
  };
}

function appClientWith(respond: (url: string, method: string) => Promise<unknown>): void {
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    const body = await respond(url, init.method ?? "GET");
    return { status: 200, url, headers: { get: () => "application/json" }, text: async () => JSON.stringify(body) };
  }) as never);
}

afterEach(() => resetAppClient());

describe("sendFailureLine", () => {
  it("prefers the server detail, then the machine code, then a sentence", () => {
    expect(
      sendFailureLine(
        new ApiError({
          status: 409,
          code: "conflict",
          message: "conflict",
          body: { error: "turn_active", detail: "A turn is running." },
        })
      )
    ).toBe("A turn is running.");
    expect(
      sendFailureLine(
        new ApiError({ status: 409, code: "conflict", message: "conflict", body: { error: "turn_active" } })
      )
    ).toBe("turn_active");
    expect(sendFailureLine(new Error("offline"))).toBe("offline");
    expect(sendFailureLine(undefined)).toBe(SEND_FAILED_FALLBACK);
  });
});

describe("useSendMessage", () => {
  it("posts the content and patches the thread list refresh", async () => {
    const message = {
      id: "22222222-2222-4222-8222-222222222222",
      role: "user",
      content: "hello",
      status: "completed",
      seq: 1,
      turn_id: null,
      payload: {},
      created_at: "2026-01-01T00:00:00Z",
      completed_at: "2026-01-01T00:00:00Z",
    };
    let seen: { url: string; method: string } | undefined;
    appClientWith(async (url, method) => {
      seen = { url, method };
      return { turn: { id: "33333333-3333-4333-8333-333333333333", status: "queued" }, message };
    });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useSendMessage(), { wrapper: wrapper(queryClient) });
    result.current.mutate({ workspaceSlug: "acme", threadId: "t1", content: "hello" });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data?.message.id).toBe(message.id);
    expect(seen).toEqual({
      url: "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/messages/",
      method: "POST",
    });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: assistantKeys.threads("acme") });
  });
});

describe("useStopTurn", () => {
  it("posts cancel and refreshes the thread subtree", async () => {
    let seen: { url: string; method: string } | undefined;
    appClientWith(async (url, method) => {
      seen = { url, method };
      return {};
    });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useStopTurn(), { wrapper: wrapper(queryClient) });
    result.current.mutate({ workspaceSlug: "acme", threadId: "t1" });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(seen).toEqual({
      url: "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/cancel/",
      method: "POST",
    });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: threadKey("acme", "t1") });
  });
});
