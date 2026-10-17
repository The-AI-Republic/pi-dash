// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { notificationKeys } from "./keys.js";
import {
  applyEmailPreferenceFlip,
  EMAIL_PREFS_FAILED_TITLE,
  EMAIL_PREFS_SAVED_TITLE,
  emailPreferencePatch,
  emailPreferencesQueryOptions,
  saveFailureMessage,
  useEmailPreferences,
  useUpdateEmailPreference,
} from "./preferences.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

const PREFS_PATH = "/api/users/me/notification-preferences/";

function prefsRow(overrides: Record<string, unknown> = {}) {
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
    ...overrides,
  };
}

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return {
    status,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  } as TransportResponse;
}

/** Stub API: CSRF token, prefs GET from a server row, prefs PATCH with one key. */
function stubApi(options: { row?: Record<string, unknown>; patchStatus?: number } = {}) {
  let server = prefsRow(options.row);
  const patches: Array<Record<string, unknown>> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return jsonResponse(url, 200, { csrf_token: "csrf" });
    }
    if (!url.endsWith(PREFS_PATH)) throw new Error(`unexpected fetch ${url}`);
    if (init.method === "PATCH") {
      const sent = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
      patches.push(sent);
      if ((options.patchStatus ?? 200) !== 200) {
        return jsonResponse(url, options.patchStatus ?? 500, { detail: "parity prefs failure" });
      }
      server = { ...server, ...sent };
      return jsonResponse(url, 200, server);
    }
    return jsonResponse(url, 200, server);
  }) as never);
  return {
    patches,
    serverRow: () => server,
  };
}

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

function testClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("email preference patch helpers", () => {
  it("sends only the flipped key", () => {
    expect(emailPreferencePatch({ topic: "comment", enabled: true })).toEqual({ comment: true });
  });

  it("flips one topic over the cached row", () => {
    const previous = prefsRow({ comment: false, mention: true });
    expect(applyEmailPreferenceFlip(previous as never, { topic: "comment", enabled: true })).toEqual({
      ...previous,
      comment: true,
    });
  });

  it("leaves an empty cache empty", () => {
    expect(applyEmailPreferenceFlip(undefined, { topic: "comment", enabled: true })).toBeUndefined();
  });

  it("keys preferences at the user level", () => {
    expect(notificationKeys.emailPreferences).toEqual(["notifications", "email-preferences"]);
  });

  it("reads the message off errors and nothing off anything else", () => {
    expect(saveFailureMessage(new Error("nope"))).toBe("nope");
    expect(saveFailureMessage("nope")).toBeUndefined();
    expect(saveFailureMessage(undefined)).toBeUndefined();
  });
});

describe("emailPreferencesQueryOptions", () => {
  it("loads the row through the preferences contract", async () => {
    const row = prefsRow();
    const getJson = vi.fn(async () => row);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = emailPreferencesQueryOptions(fakeClient);
    expect(options.queryKey).toEqual(notificationKeys.emailPreferences);
    expect(options.staleTime).toBe(REFERENCE_STALE_TIME_MS);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(row);
    expect(getJson).toHaveBeenCalledWith(PREFS_PATH);
  });
});

describe("useEmailPreferences", () => {
  it("reads the server row", async () => {
    stubApi({ row: { comment: true } });
    const queryClient = testClient();
    const { result } = renderHook(() => useEmailPreferences(), { wrapper: wrapper(queryClient) });
    expect(result.current.isPending).toBe(true);
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data?.comment).toBe(true);
    expect(result.current.data?.mention).toBe(true);
  });
});

describe("useUpdateEmailPreference", () => {
  it("PATCHes the single changed key and toasts success", async () => {
    const api = stubApi();
    const queryClient = testClient();
    queryClient.setQueryData(notificationKeys.emailPreferences, prefsRow());
    const { result } = renderHook(() => useUpdateEmailPreference(), { wrapper: wrapper(queryClient) });

    let saved: unknown;
    await act(async () => {
      saved = await result.current.mutateAsync({ topic: "comment", enabled: true });
    });

    expect(api.patches).toEqual([{ comment: true }]);
    expect(api.serverRow().comment).toBe(true);
    expect((saved as { comment: boolean }).comment).toBe(true);
    expect(queryClient.getQueryData(notificationKeys.emailPreferences)).toMatchObject({ comment: true });
    expect(notifySuccess).toHaveBeenCalledWith(EMAIL_PREFS_SAVED_TITLE);
    expect(notifySuccess).toHaveBeenCalledTimes(1);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("moves the toggle before the save settles", async () => {
    let release!: (row: Record<string, unknown>) => void;
    const gate = new Promise<Record<string, unknown>>((resolve) => {
      release = resolve;
    });
    createAppClient("https://api.test", (async (url: string, _init: { method?: string; body?: string }) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return jsonResponse(url, 200, { csrf_token: "csrf" });
      }
      const row = await gate;
      return jsonResponse(url, 200, row);
    }) as never);
    const queryClient = testClient();
    queryClient.setQueryData(notificationKeys.emailPreferences, prefsRow({ comment: false }));
    const { result } = renderHook(() => useUpdateEmailPreference(), { wrapper: wrapper(queryClient) });

    let pending: Promise<unknown> | null = null;
    await act(async () => {
      pending = result.current.mutateAsync({ topic: "comment", enabled: true });
    });
    // The PATCH is still gated: the cache already shows the flip.
    expect(queryClient.getQueryData(notificationKeys.emailPreferences)).toMatchObject({ comment: true });

    await act(async () => {
      release(prefsRow({ comment: true }));
      await pending;
    });
    expect(queryClient.getQueryData(notificationKeys.emailPreferences)).toMatchObject({ comment: true });
    expect(notifySuccess).toHaveBeenCalledWith(EMAIL_PREFS_SAVED_TITLE);
  });

  it("rolls back to the persisted value with an error toast when the save fails", async () => {
    const api = stubApi({ patchStatus: 500 });
    const queryClient = testClient();
    const before = prefsRow({ comment: false });
    queryClient.setQueryData(notificationKeys.emailPreferences, before);
    const { result } = renderHook(() => useUpdateEmailPreference(), { wrapper: wrapper(queryClient) });

    await act(async () => {
      await expect(result.current.mutateAsync({ topic: "comment", enabled: true })).rejects.toThrow();
    });

    expect(api.patches).toEqual([{ comment: true }]);
    expect(api.serverRow().comment).toBe(false);
    expect(queryClient.getQueryData(notificationKeys.emailPreferences)).toMatchObject({ comment: false });
    expect(notifyError).toHaveBeenCalledWith(EMAIL_PREFS_FAILED_TITLE, expect.any(String));
    expect(notifySuccess).not.toHaveBeenCalled();
  });
});
