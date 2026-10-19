// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ToastHost } from "@pidash/kit";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { getToastManager } from "../../../shared/shell/index.js";
import {
  appendPersonalToken,
  personalTokenKeys,
  removePersonalToken,
  tokenFailureDetail,
  useCreatePersonalToken,
  useDeletePersonalToken,
  usePersonalTokens,
} from "./api.js";
import { TOKEN_CREATE_FAILED_TITLE, TOKEN_DELETE_FAILED_TITLE, TOKEN_DELETED_NOTICE } from "./model.js";

const TOKEN_ID = "123e4567-e89b-12d3-a456-426614174000";
const USER_ID = "223e4567-e89b-12d3-a456-426614174001";

function token(overrides: Record<string, unknown> = {}) {
  return {
    id: TOKEN_ID,
    label: "Deploy bot",
    description: "Nightly deploy",
    is_active: true,
    last_used: null,
    user: USER_ID,
    user_type: 0,
    workspace: null,
    expired_at: null,
    is_service: false,
    allowed_rate_limit: "60/minute",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: USER_ID,
    updated_by: USER_ID,
    ...overrides,
  };
}

interface Reply {
  status: number;
  body: string;
}

function json(url: string, body: string, status = 200) {
  return { status, url, headers: { get: () => "application/json" }, text: async () => body };
}

function harness(reply: (url: string, method: string) => Reply | undefined) {
  const calls: Array<{ url: string; method: string; body?: string | undefined }> = [];
  createAppClient("https://api.test", (async (url: string, init: { method: string; body?: string }) => {
    calls.push({ url, method: init.method, body: init.body });
    if (url.endsWith("/auth/get-csrf-token/")) return json(url, '{"csrf_token":"csrf"}');
    const answer = reply(url, init.method);
    if (answer === undefined) throw new Error(`unexpected call ${init.method} ${url}`);
    return json(url, answer.body, answer.status);
  }) as never);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <React.Fragment>{children}</React.Fragment>
      <ToastHost manager={getToastManager()} />
    </QueryClientProvider>
  );
  return { calls, queryClient, wrapper };
}

afterEach(() => {
  resetAppClient();
});

describe("cache helpers", () => {
  it("prepends a created row and avoids duplicating an existing one", () => {
    const created = token();
    expect(appendPersonalToken(undefined, created)).toEqual([created]);
    expect(appendPersonalToken([created], created)).toEqual([created]);
    expect(appendPersonalToken(undefined, created)[0]).toBe(created);
  });

  it("removes a row and tolerates a missing cache", () => {
    expect(removePersonalToken([token()], TOKEN_ID)).toEqual([]);
    expect(removePersonalToken(undefined, TOKEN_ID)).toBeUndefined();
  });

  it("reports a server detail when one exists, otherwise a plain message", () => {
    expect(tokenFailureDetail(new Error("Network down"))).toBe("Network down");
    expect(tokenFailureDetail(new Error(""))).toBeUndefined();
    expect(tokenFailureDetail("nope")).toBeUndefined();
  });

  it("prepends to an existing list and returns a distinct array", () => {
    const previous = [token({ id: "323e4567-e89b-12d3-a456-426614174002", label: "Older" })];
    const created = token();
    const next = appendPersonalToken(previous, created);
    expect(next).toEqual([created, ...previous]);
    expect(next).not.toBe(previous);
  });
});

describe("usePersonalTokens", () => {
  it("reads the caller's tokens", async () => {
    const { wrapper } = harness((url, method) =>
      method === "GET" && url === "https://api.test/api/users/api-tokens/"
        ? { status: 200, body: JSON.stringify([token()]) }
        : undefined
    );
    const { result } = renderHook(() => usePersonalTokens(), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual([token()]);
  });
});

describe("useCreatePersonalToken", () => {
  it("posts the contract payload, prepends the answer, and keeps the secret for the reveal", async () => {
    const created = { ...token(), token: "pi_dash_api_abc" };
    const { calls, queryClient, wrapper } = harness((url, method) =>
      method === "POST" && url === "https://api.test/api/users/api-tokens/"
        ? { status: 201, body: JSON.stringify(created) }
        : undefined
    );
    queryClient.setQueryData(personalTokenKeys.list, []);
    const { result } = renderHook(() => useCreatePersonalToken(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({
        label: "Deploy bot",
        description: "Nightly deploy",
        expired_at: null,
      });
    });
    expect(JSON.parse(calls.find((call) => call.method === "POST")?.body ?? "{}")).toEqual({
      label: "Deploy bot",
      description: "Nightly deploy",
      expired_at: null,
    });
    expect(queryClient.getQueryData(personalTokenKeys.list)).toEqual([created]);
  });

  it("reports a failed create and leaves the cache untouched", async () => {
    const { queryClient, wrapper } = harness((url, method) =>
      method === "POST" && url.endsWith("/api/users/api-tokens/")
        ? { status: 400, body: '{"detail":"Title is required"}' }
        : undefined
    );
    queryClient.setQueryData(personalTokenKeys.list, []);
    const { result } = renderHook(() => useCreatePersonalToken(), { wrapper });
    await act(async () => {
      await expect(result.current.mutateAsync({ label: "", description: "", expired_at: null })).rejects.toMatchObject({
        status: 400,
      });
    });
    expect(queryClient.getQueryData(personalTokenKeys.list)).toEqual([]);
    expect(TOKEN_CREATE_FAILED_TITLE).toBeTruthy();
  });
});

describe("useDeletePersonalToken", () => {
  it("optimistically removes the row, deletes with no trailing slash, and confirms", async () => {
    const { calls, queryClient, wrapper } = harness((url, method) =>
      method === "DELETE" && url === `https://api.test/api/users/api-tokens/${TOKEN_ID}`
        ? { status: 204, body: "" }
        : undefined
    );
    queryClient.setQueryData(personalTokenKeys.list, [token()]);
    const { result } = renderHook(() => useDeletePersonalToken(), { wrapper });
    act(() => result.current.mutate(TOKEN_ID));
    await waitFor(() => expect(queryClient.getQueryData(personalTokenKeys.list)).toEqual([]));
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls.some((call) => call.method === "DELETE")).toBe(true);
    expect(TOKEN_DELETED_NOTICE).toBeTruthy();
  });

  it("restores the row when the delete fails", async () => {
    const { queryClient, wrapper } = harness((url, method) =>
      method === "DELETE" && url.endsWith(`/api/users/api-tokens/${TOKEN_ID}`)
        ? { status: 500, body: '{"detail":"boom"}' }
        : undefined
    );
    queryClient.setQueryData(personalTokenKeys.list, [token()]);
    const { result } = renderHook(() => useDeletePersonalToken(), { wrapper });
    await act(async () => {
      await expect(result.current.mutateAsync(TOKEN_ID)).rejects.toBeTruthy();
    });
    expect(queryClient.getQueryData(personalTokenKeys.list)).toEqual([token()]);
    expect(TOKEN_DELETE_FAILED_TITLE).toBeTruthy();
  });
});
