// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, renderHook, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { useSessionStore } from "../../core/session/store.js";
import {
  useForgotPassword,
  useMagicSignIn,
  useMagicSignUp,
  usePasswordReset,
  usePasswordSignIn,
  usePasswordSignUp,
  useSetPassword,
  useSignOut,
} from "./api.js";

vi.mock("../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError } from "../../shared/shell/index.js";
import { SIGN_OUT_FAILED_MESSAGE, SIGN_OUT_FAILED_TITLE } from "./signOut.js";

interface SeenRequest {
  url: string;
  csrf: string | null;
}

const CSRF_URL = "https://api.test/auth/get-csrf-token/";

function recordingTransport(seen: SeenRequest[]): Transport {
  return (url, init) => {
    seen.push({ url, csrf: init.headers["X-CSRFTOKEN"] ?? null });
    if (url === CSRF_URL) {
      return Promise.resolve({
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ csrf_token: "fresh-token" })),
      });
    }
    let body = "";
    if (url.endsWith("/auth/forgot-password/")) {
      body = JSON.stringify({ message: "Check your email to reset your password" });
    } else if (url.endsWith("/auth/set-password/")) {
      body = JSON.stringify({
        id: "11111111-1111-4111-8111-111111111111",
        email: "ada@example.com",
        is_password_autoset: false,
      });
    }
    return Promise.resolve({
      status: 200,
      url: "https://api.test/landing",
      headers: { get: () => null },
      text: () => Promise.resolve(body),
    });
  };
}

function wrapper(client: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Provider({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={client}>{children as never}</QueryClientProvider>;
  };
}

interface StubRoute {
  status: number;
  body: string;
  url: string;
}

function stubTransport(handler: (url: string, init: TransportInit) => StubRoute | Promise<StubRoute>): Transport {
  return async (url, init) => {
    const response = await handler(url, init);
    return {
      status: response.status,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
}

const CSRF = JSON.stringify({ csrf_token: "csrf" });

interface Settlement {
  landing?: string | undefined;
  error?: unknown;
}

function SignOutProbe({ onSettled }: { onSettled: (outcome: Settlement) => void }): React.ReactElement {
  const signOut = useSignOut();
  return (
    <button
      type="button"
      onClick={() => {
        signOut.mutate(undefined, {
          onSuccess: (landing) => onSettled({ landing }),
          onError: (error) => onSettled({ error }),
        });
      }}
    >
      Sign out
    </button>
  );
}

function renderProbe(transport: Transport, onSettled: (outcome: Settlement) => void): QueryClient {
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  queryClient.setQueryData(["session", "me"], { id: "user-1" });
  render(
    <QueryClientProvider client={queryClient}>
      <SignOutProbe onSettled={onSettled} />
    </QueryClientProvider>
  );
  return queryClient;
}

afterEach(() => {
  resetAppClient();
  vi.mocked(notifyError).mockClear();
  useSessionStore.setState({ status: "authenticated", returnUrl: null });
});

/**
 * Every credential submission drops the cached forgery token first, so the
 * POST carries a freshly fetched one even when the cache is warm from an
 * earlier submit (AUTH-020). Each entry submits twice and counts the
 * token fetches.
 */
describe.each([
  {
    name: "usePasswordSignIn",
    useHook: usePasswordSignIn,
    args: { email: "ada@example.com", password: "secret-1" },
    postPath: "/auth/sign-in/",
  },
  {
    name: "useMagicSignIn",
    useHook: useMagicSignIn,
    args: { email: "ada@example.com", code: "123456" },
    postPath: "/auth/magic-sign-in/",
  },
  {
    name: "usePasswordSignUp",
    useHook: usePasswordSignUp,
    args: { email: "ada@example.com", password: "secret-1", confirmPassword: "secret-1" },
    postPath: "/auth/sign-up/",
  },
  {
    name: "useMagicSignUp",
    useHook: useMagicSignUp,
    args: { email: "ada@example.com", code: "123456" },
    postPath: "/auth/magic-sign-up/",
  },
  {
    name: "useForgotPassword",
    useHook: useForgotPassword,
    args: "ada@example.com",
    postPath: "/auth/forgot-password/",
  },
  {
    name: "usePasswordReset",
    useHook: usePasswordReset,
    args: { uid: "uid", token: "token", password: "Tq7!mZ2@kP9#xR", confirmPassword: "Tq7!mZ2@kP9#xR" },
    postPath: "/auth/reset-password/uid/token/",
  },
  {
    name: "useSetPassword",
    useHook: useSetPassword,
    args: "Tq7!mZ2@kP9#xR",
    postPath: "/auth/set-password/",
  },
])(
  "$name",
  ({
    useHook,
    args,
    postPath,
  }: {
    useHook: () => { mutateAsync: (input: never) => Promise<unknown> };
    args: unknown;
    postPath: string;
  }) => {
    it("fetches a fresh forgery token for every submission", async () => {
      const seen: SeenRequest[] = [];
      createAppClient("https://api.test", recordingTransport(seen));
      const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      const { result } = renderHook(useHook, { wrapper: wrapper(queryClient) });

      await act(async () => {
        await result.current.mutateAsync(args as never);
      });
      await act(async () => {
        await result.current.mutateAsync(args as never);
      });

      const tokenFetches = seen.filter((request) => request.url === CSRF_URL);
      expect(tokenFetches).toHaveLength(2);
      const posts = seen.filter((request) => request.url.endsWith(postPath));
      expect(posts).toHaveLength(2);
      for (const post of posts) {
        expect(post.csrf).toBe("fresh-token");
      }
    });
  }
);

describe("useSignOut", () => {
  it("signs out to the entry route", async () => {
    const settled: Settlement[] = [];
    const queryClient = renderProbe(
      stubTransport(async (url) => ({
        status: 200,
        body: url.endsWith("/auth/get-csrf-token/") ? CSRF : "{}",
        url,
      })),
      (outcome) => settled.push(outcome)
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Sign out" }));

    await waitFor(() => expect(settled).toHaveLength(1));
    expect(settled[0]).toEqual({ landing: "/" });
    expect(queryClient.getQueryData(["session", "me"])).toBeUndefined();
    expect(useSessionStore.getState().status).toBe("signed-out");
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("toasts and rejects a failed POST without clearing", async () => {
    const settled: Settlement[] = [];
    const queryClient = renderProbe(
      stubTransport(async (url) => {
        if (url.endsWith("/auth/sign-out/")) return { status: 500, body: "{}", url };
        return { status: 200, body: CSRF, url };
      }),
      (outcome) => settled.push(outcome)
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Sign out" }));

    await waitFor(() => expect(settled).toHaveLength(1));
    expect(settled[0]?.error).toBeInstanceOf(Error);
    expect(notifyError).toHaveBeenCalledWith(SIGN_OUT_FAILED_TITLE, SIGN_OUT_FAILED_MESSAGE);
    expect(queryClient.getQueryData(["session", "me"])).toEqual({ id: "user-1" });
    expect(useSessionStore.getState().status).toBe("authenticated");
  });
});
