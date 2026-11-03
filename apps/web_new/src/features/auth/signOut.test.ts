// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { registerRuntimeSignOutHooks } from "../../core/session/index.js";
import { registerStoreReset, useSessionStore } from "../../core/session/store.js";

vi.mock("../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn() };
});

import { notifyError } from "../../shared/shell/index.js";
import { EXPLICIT_LOGOUT_KEY } from "./cloudLogin.js";
import {
  performSignOut,
  SIGN_OUT_FAILED_MESSAGE,
  SIGN_OUT_FAILED_TITLE,
  signOutLandingTarget,
  useSignOut,
} from "./signOut.js";

beforeEach(() => {
  useSessionStore.setState({ status: "authenticated", returnUrl: null });
  vi.mocked(notifyError).mockClear();
});

afterEach(() => {
  resetAppClient();
  vi.unstubAllGlobals();
});

function stubClient(post: () => Promise<unknown>) {
  return {
    clearCsrfCache: vi.fn(),
    post: vi.fn(post),
  };
}

describe("signOutLandingTarget", () => {
  it("lands on the entry route on web and the login route on desktop", () => {
    expect(signOutLandingTarget("web")).toBe("/");
    expect(signOutLandingTarget("desktop")).toBe("/login");
  });
});

describe("performSignOut", () => {
  it("posts with a fresh token, then clears every store and the cache", async () => {
    const client = stubClient(async () => ({ url: "https://app.test/" }));
    const queryClient = new QueryClient();
    queryClient.setQueryData(["session", "me"], { id: "user-1" });
    const reset = vi.fn();
    const unregister = registerStoreReset(reset);

    try {
      await expect(performSignOut({ client: client as never, queryClient })).resolves.toBe("/");
    } finally {
      unregister();
    }

    const clearOrder = vi.mocked(client.clearCsrfCache).mock.invocationCallOrder[0] ?? 0;
    const postOrder = vi.mocked(client.post).mock.invocationCallOrder[0] ?? 0;
    expect(clearOrder).toBeLessThan(postOrder);
    expect(client.post).toHaveBeenCalledWith("/auth/sign-out/", {});
    expect(reset).toHaveBeenCalledTimes(1);
    expect(queryClient.getQueryData(["session", "me"])).toBeUndefined();
    expect(useSessionStore.getState().status).toBe("signed-out");
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("toasts and rethrows a failed POST without clearing anything", async () => {
    const client = stubClient(async () => {
      throw new Error("boom");
    });
    const queryClient = new QueryClient();
    queryClient.setQueryData(["session", "me"], { id: "user-1" });

    await expect(performSignOut({ client: client as never, queryClient })).rejects.toThrow("boom");
    expect(notifyError).toHaveBeenCalledWith(SIGN_OUT_FAILED_TITLE, SIGN_OUT_FAILED_MESSAGE);
    expect(queryClient.getQueryData(["session", "me"])).toEqual({ id: "user-1" });
    expect(useSessionStore.getState().status).toBe("authenticated");
  });

  it("runs the desktop teardown after the POST and before landing", async () => {
    const client = stubClient(async () => ({ url: "https://app.test/" }));
    const queryClient = new QueryClient();
    const clearLocalSession = vi.fn(async () => undefined);

    await expect(
      performSignOut({ client: client as never, queryClient, teardown: { clearLocalSession } })
    ).resolves.toBe("/");

    const postOrder = vi.mocked(client.post).mock.invocationCallOrder[0] ?? 0;
    const wipeOrder = clearLocalSession.mock.invocationCallOrder[0] ?? 0;
    expect(postOrder).toBeLessThan(wipeOrder);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("records the tab-scoped explicit-logout marker after a successful POST", async () => {
    const entries = new Map<string, string>();
    vi.stubGlobal("sessionStorage", {
      getItem: (key: string) => entries.get(key) ?? null,
      setItem: (key: string, value: string) => void entries.set(key, value),
      removeItem: (key: string) => void entries.delete(key),
    });
    const client = stubClient(async () => ({ url: "https://app.test/" }));

    await expect(performSignOut({ client: client as never, queryClient: new QueryClient() })).resolves.toBe("/");
    expect(entries.get(EXPLICIT_LOGOUT_KEY)).toBe("1");
  });

  it("writes the marker after the desktop teardown wipes the same storage", async () => {
    const entries = new Map<string, string>();
    vi.stubGlobal("sessionStorage", {
      getItem: (key: string) => entries.get(key) ?? null,
      setItem: (key: string, value: string) => void entries.set(key, value),
      removeItem: (key: string) => void entries.delete(key),
    });
    const client = stubClient(async () => ({ url: "https://app.test/" }));
    const clearLocalSession = vi.fn(async () => {
      entries.clear();
    });

    await expect(
      performSignOut({ client: client as never, queryClient: new QueryClient(), teardown: { clearLocalSession } })
    ).resolves.toBe("/");

    expect(clearLocalSession).toHaveBeenCalledTimes(1);
    expect(entries.get(EXPLICIT_LOGOUT_KEY)).toBe("1");
  });

  it("leaves no marker when the POST failed", async () => {
    const entries = new Map<string, string>();
    vi.stubGlobal("sessionStorage", {
      getItem: (key: string) => entries.get(key) ?? null,
      setItem: (key: string, value: string) => void entries.set(key, value),
      removeItem: (key: string) => void entries.delete(key),
    });
    const client = stubClient(async () => {
      throw new Error("boom");
    });

    await expect(performSignOut({ client: client as never, queryClient: new QueryClient() })).rejects.toThrow("boom");
    expect(entries.has(EXPLICIT_LOGOUT_KEY)).toBe(false);
  });

  it("signs out when storage access throws", async () => {
    Object.defineProperty(globalThis, "sessionStorage", {
      configurable: true,
      get() {
        throw new Error("denied");
      },
    });
    const client = stubClient(async () => ({ url: "https://app.test/" }));
    await expect(performSignOut({ client: client as never, queryClient: new QueryClient() })).resolves.toBe("/");
  });

  it("signs out when the runtime has no session storage", async () => {
    vi.stubGlobal("sessionStorage", undefined);
    const client = stubClient(async () => ({ url: "https://app.test/" }));
    await expect(performSignOut({ client: client as never, queryClient: new QueryClient() })).resolves.toBe("/");
  });

  it("swallows a failed local teardown and still lands", async () => {
    const client = stubClient(async () => ({ url: "https://app.test/" }));
    const queryClient = new QueryClient();
    const clearLocalSession = vi.fn(async () => {
      throw new Error("wipe failed");
    });

    await expect(
      performSignOut({ client: client as never, queryClient, teardown: { clearLocalSession } })
    ).resolves.toBe("/");
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("disposes the runtime before the POST once confirmed", async () => {
    const client = stubClient(async () => ({ url: "https://app.test/" }));
    const queryClient = new QueryClient();
    const choice = { deleteChatHistory: true };
    const confirmRuntimeSignOut = vi.fn(async () => choice);
    const disposeRuntimeForSignOut = vi.fn(async () => undefined);
    const unregister = registerRuntimeSignOutHooks({ confirmRuntimeSignOut, disposeRuntimeForSignOut });

    try {
      await expect(performSignOut({ client: client as never, queryClient })).resolves.toBe("/");
    } finally {
      unregister();
    }

    expect(confirmRuntimeSignOut).toHaveBeenCalledTimes(1);
    expect(disposeRuntimeForSignOut).toHaveBeenCalledWith(choice);
    const disposeOrder = disposeRuntimeForSignOut.mock.invocationCallOrder[0] ?? 0;
    const postOrder = vi.mocked(client.post).mock.invocationCallOrder[0] ?? 0;
    expect(disposeOrder).toBeGreaterThan(0);
    expect(disposeOrder).toBeLessThan(postOrder);
    expect(useSessionStore.getState().status).toBe("signed-out");
  });

  it("signs nothing out when the runtime confirmation is cancelled", async () => {
    const client = stubClient(async () => ({ url: "https://app.test/" }));
    const queryClient = new QueryClient();
    queryClient.setQueryData(["session", "me"], { id: "user-1" });
    const confirmRuntimeSignOut = vi.fn(async () => null);
    const disposeRuntimeForSignOut = vi.fn(async () => undefined);
    const unregister = registerRuntimeSignOutHooks({ confirmRuntimeSignOut, disposeRuntimeForSignOut });

    try {
      await expect(performSignOut({ client: client as never, queryClient })).resolves.toBeNull();
    } finally {
      unregister();
    }

    expect(disposeRuntimeForSignOut).not.toHaveBeenCalled();
    expect(client.post).not.toHaveBeenCalled();
    expect(queryClient.getQueryData(["session", "me"])).toEqual({ id: "user-1" });
    expect(useSessionStore.getState().status).toBe("authenticated");
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("disposes without asking for already-confirmed sign-outs", async () => {
    const client = stubClient(async () => ({ url: "https://app.test/" }));
    const queryClient = new QueryClient();
    const confirmRuntimeSignOut = vi.fn(async () => ({ deleteChatHistory: true }));
    const disposeRuntimeForSignOut = vi.fn(async () => undefined);
    const unregister = registerRuntimeSignOutHooks({ confirmRuntimeSignOut, disposeRuntimeForSignOut });

    try {
      await expect(performSignOut({ client: client as never, queryClient, skipRuntimeConfirm: true })).resolves.toBe(
        "/"
      );
    } finally {
      unregister();
    }

    expect(confirmRuntimeSignOut).not.toHaveBeenCalled();
    expect(disposeRuntimeForSignOut).toHaveBeenCalledWith({ deleteChatHistory: false });
    expect(client.post).toHaveBeenCalledWith("/auth/sign-out/", {});
  });

  it("exposes the sign-out through the app client and platform teardown", async () => {
    createAppClient("https://app.test", async (url) => ({
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => (url.endsWith("/auth/get-csrf-token/") ? JSON.stringify({ csrf_token: "csrf" }) : "{}"),
    }));
    const wrapper = ({ children }: { children: React.ReactNode }) =>
      React.createElement(QueryClientProvider, { client: new QueryClient() }, children);
    const { result } = renderHook(() => useSignOut(), { wrapper });

    await expect(result.current.mutateAsync()).resolves.toBe("/");
    expect(useSessionStore.getState().status).toBe("signed-out");
    expect(notifyError).not.toHaveBeenCalled();
  });
});

describe("isHostedBackend", () => {
  it("reads the hosted marker off the cached instance config", async () => {
    const { isHostedBackend } = await import("./signOut.js");
    const { instanceKeys } = await import("../../core/session/index.js");
    const hosted = new QueryClient();
    hosted.setQueryData(instanceKeys.config, { is_self_managed: false });
    expect(isHostedBackend(hosted)).toBe(true);
    const managed = new QueryClient();
    managed.setQueryData(instanceKeys.config, { is_self_managed: true });
    expect(isHostedBackend(managed)).toBe(false);
    expect(isHostedBackend(new QueryClient())).toBe(false);
  });
});

describe("performSignOut on the hosted backend", () => {
  function cloudClient() {
    return {
      clearCsrfCache: vi.fn(),
      getJson: vi.fn(async () => ({ csrf_token: "csrf" })),
      sendJson: vi.fn(async () => ({
        data: { airepublic_logout_url: "https://accounts.test/logout" },
        status: 200,
        url: "https://api.test/api/auth/sign-out/",
      })),
      parse: vi.fn((_schema: unknown, data: unknown) => data),
    };
  }

  function hostedQueryClient(): QueryClient {
    const queryClient = new QueryClient();
    queryClient.setQueryData(["instance", "config"], { is_self_managed: false });
    queryClient.setQueryData(["session", "me"], { id: "user-1" });
    return queryClient;
  }

  function stubLocation(): { replace: ReturnType<typeof vi.fn> } {
    const replace = vi.fn();
    Object.defineProperty(globalThis.window, "location", {
      value: { replace },
      writable: true,
      configurable: true,
    });
    return { replace };
  }

  it("posts the cloud logout pair and replaces onto the login card", async () => {
    const entries = new Map<string, string>();
    vi.stubGlobal("sessionStorage", {
      getItem: (key: string) => entries.get(key) ?? null,
      setItem: (key: string, value: string) => void entries.set(key, value),
      removeItem: (key: string) => void entries.delete(key),
    });
    const { replace } = stubLocation();
    const client = cloudClient();
    const queryClient = hostedQueryClient();

    await expect(performSignOut({ client: client as never, queryClient })).resolves.toBe("/login");

    expect(client.getJson).toHaveBeenCalledWith("/api/auth/get-csrf-token/", { csrf: false });
    expect(client.sendJson).toHaveBeenCalledWith("POST", "/api/auth/sign-out/", {
      json: {},
      headers: { "X-CSRFTOKEN": "csrf" },
    });
    expect(queryClient.getQueryData(["session", "me"])).toBeUndefined();
    expect(entries.get(EXPLICIT_LOGOUT_KEY)).toBe("1");
    expect(replace).toHaveBeenCalledWith("/login");
    expect(replace).toHaveBeenCalledTimes(1);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("toasts and rethrows a failed cloud POST without navigating", async () => {
    const entries = new Map<string, string>();
    vi.stubGlobal("sessionStorage", {
      getItem: (key: string) => entries.get(key) ?? null,
      setItem: (key: string, value: string) => void entries.set(key, value),
      removeItem: (key: string) => void entries.delete(key),
    });
    const { replace } = stubLocation();
    const client = cloudClient();
    client.sendJson.mockRejectedValueOnce(new Error("boom"));
    const queryClient = hostedQueryClient();

    await expect(performSignOut({ client: client as never, queryClient })).rejects.toThrow("boom");
    expect(notifyError).toHaveBeenCalledWith(SIGN_OUT_FAILED_TITLE, SIGN_OUT_FAILED_MESSAGE);
    expect(replace).not.toHaveBeenCalled();
    expect(entries.has(EXPLICIT_LOGOUT_KEY)).toBe(false);
    expect(queryClient.getQueryData(["session", "me"])).toEqual({ id: "user-1" });
  });
});
