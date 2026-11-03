// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-owned provider sign-in (CLOUD-033): the reserved tab, every distinct
// start and poll outcome, the refresh after a confirmation, and the poll
// stopping when the page goes away.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { notifySuccess, notifyError } = vi.hoisted(() => ({
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

vi.mock("../../../shared/shell/toaster.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/toaster.js")>();
  return { ...original, notifySuccess, notifyInfo: vi.fn(), notifyError };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { cloudAssistantKeys } from "../api/keys.js";
import { SIGN_IN_POLL_INTERVAL_MS, type PollClock, type PopupLike } from "../lib/signIn.js";
import { useProviderSignIn } from "./useProviderSignIn.js";

function manualClock() {
  let now = 0;
  const callbacks: Array<() => void> = [];
  const cleared: unknown[] = [];
  const clock: PollClock = {
    now: () => now,
    setInterval: (callback) => {
      callbacks.push(callback);
      return callbacks.length;
    },
    clearInterval: (handle) => cleared.push(handle),
  };
  return {
    clock,
    callbacks,
    cleared,
    advance(ms: number) {
      now += ms;
    },
    async fire() {
      const callback = callbacks[0];
      if (callback === undefined) throw new Error("no interval registered");
      callback();
      for (let turn = 0; turn < 8; turn += 1) await Promise.resolve();
    },
  };
}

function popup(): PopupLike & { replaced: string[] } {
  const replaced: string[] = [];
  return {
    closed: false,
    opener: {},
    location: { replace: (url: string) => replaced.push(url) },
    close: vi.fn(),
    replaced,
  };
}

interface Call {
  method: string;
  url: string;
}

type Reply = { status: number; body?: unknown } | undefined;

/** A stub whose answers a test can change between polls. */
function stubTransport(answers: Record<string, Reply>): { calls: Call[]; answers: Record<string, Reply> } {
  const calls: Call[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    const method = init.method ?? "GET";
    calls.push({ method, url });
    const key = Object.keys(answers)
      .sort((left, right) => right.length - left.length)
      .find((candidate) => url.includes(candidate));
    const reply = key === undefined ? undefined : answers[key];
    if (reply === undefined) throw new Error(`unexpected call ${method} ${url}`);
    const text = reply.body === undefined ? "" : JSON.stringify(reply.body);
    return {
      status: reply.status,
      url,
      headers: { get: () => (reply.body === undefined ? null : "application/json") },
      text: async () => text,
    };
  }) as never);
  return { calls, answers };
}

function setup(
  overrides: { open?: () => PopupLike | null; clock?: PollClock; intervalMs?: number; onConfirmed?: () => void } = {}
) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const wrapper = ({ children }: { children?: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>
  );
  const rendered = renderHook(() => useProviderSignIn({ open: overrides.open ?? popup, ...overrides }), { wrapper });
  return { ...rendered, queryClient };
}

const START = "/api/users/me/ai-assistant/openai-login/";
const STATUS = "/api/users/me/ai-assistant/openai-login/status/";

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("useProviderSignIn", () => {
  it("reports a blocked popup before starting the server flow", async () => {
    const { calls } = stubTransport({ [START]: { status: 200, body: { authorize_url: "https://auth", state: "s" } } });
    const { result } = setup({ open: () => null });
    await act(async () => {
      await result.current.signIn();
    });
    expect(notifyError).toHaveBeenCalledWith(
      "Popup blocked",
      "The sign-in window could not open. Allow pop-ups for this site and try again."
    );
    expect(calls).toEqual([]);
    expect(result.current.pending).toBe(false);
  });

  it("reports a tab closed before the start answered", async () => {
    const tab = popup();
    stubTransport({
      [START]: {
        status: 200,
        body: { authorize_url: "https://auth", state: "s" },
      },
    });
    const { result } = setup({
      open: () => tab,
    });
    tab.closed = true;
    await act(async () => {
      await result.current.signIn();
    });
    expect(notifyError).toHaveBeenCalledWith(
      "Sign-in window closed",
      "The sign-in window closed before OpenAI answered. Start it again."
    );
  });

  it("reports a rejected start with the server's detail and closes the empty tab", async () => {
    const tab = popup();
    stubTransport({ [START]: { status: 400, body: { detail: "Codex sign-in is off" } } });
    const { result } = setup({ open: () => tab });
    await act(async () => {
      await result.current.signIn();
    });
    expect(notifyError).toHaveBeenCalledWith("ChatGPT sign-in could not open", "Codex sign-in is off");
    expect(tab.close).toHaveBeenCalledTimes(1);
    expect(tab.replaced).toEqual([]);
    expect(result.current.pending).toBe(false);
  });

  it("navigates the popup, polls the state, and confirms with a refresh", async () => {
    const tab = popup();
    const { calls, answers } = stubTransport({
      [START]: { status: 200, body: { authorize_url: "https://auth.openai.com/x", state: "state-1" } },
      [STATUS]: { status: 200, body: { status: "pending" } },
    });
    const time = manualClock();
    const onConfirmed = vi.fn();
    const { result, queryClient } = setup({ open: () => tab, clock: time.clock, onConfirmed });
    queryClient.setQueryData(cloudAssistantKeys.connections(), { connections: [] });
    await act(async () => {
      await result.current.signIn();
    });
    expect(tab.replaced).toEqual(["https://auth.openai.com/x"]);
    expect(calls.some((call) => call.method === "POST" && call.url.endsWith(START))).toBe(true);
    expect(result.current.pending).toBe(true);
    await time.fire();
    expect(notifySuccess).not.toHaveBeenCalled();
    // The pending answer keeps the flow alive; the next read settles it.
    answers[STATUS] = { status: 200, body: { status: "ok" } };
    await time.fire();
    await waitFor(() =>
      expect(notifySuccess).toHaveBeenCalledWith(
        "OpenAI account ready",
        "The OpenAI account is connected, and the assistant will draw on its ChatGPT plan allowance."
      )
    );
    expect(calls.some((call) => call.url.includes("state=state-1"))).toBe(true);
    expect(onConfirmed).toHaveBeenCalledTimes(1);
    await act(async () => undefined);
    expect(result.current.pending).toBe(false);
    expect(queryClient.getQueryState(cloudAssistantKeys.connections())?.isInvalidated).toBe(true);
  });

  it("reports an expired login as a provider-side failure", async () => {
    stubTransport({
      [START]: { status: 200, body: { authorize_url: "https://auth", state: "state-2" } },
      [STATUS]: { status: 200, body: { status: "expired" } },
    });
    const time = manualClock();
    const { result } = setup({ clock: time.clock });
    await act(async () => {
      await result.current.signIn();
    });
    await time.fire();
    expect(notifyError).toHaveBeenCalledWith(
      "OpenAI sign-in failed",
      "OpenAI reported that the sign-in could not finish. Start it again."
    );
    expect(notifySuccess).not.toHaveBeenCalled();
    await act(async () => undefined);
    expect(result.current.pending).toBe(false);
  });

  it("times out when the provider never answers", async () => {
    stubTransport({
      [START]: { status: 200, body: { authorize_url: "https://auth", state: "state-3" } },
      [STATUS]: { status: 200, body: { status: "pending" } },
    });
    const time = manualClock();
    const { result } = setup({ clock: time.clock });
    await act(async () => {
      await result.current.signIn();
    });
    time.advance(5 * 60_000 + 1);
    await time.fire();
    expect(notifyError).toHaveBeenCalledWith(
      "ChatGPT sign-in did not finish",
      "No answer arrived from OpenAI before the five-minute window ended."
    );
    await act(async () => undefined);
    expect(result.current.pending).toBe(false);
    expect(time.cleared).toHaveLength(1);
  });

  it("stops polling when the page unmounts", async () => {
    stubTransport({
      [START]: { status: 200, body: { authorize_url: "https://auth", state: "state-4" } },
      [STATUS]: { status: 200, body: { status: "ok" } },
    });
    const time = manualClock();
    const { result, unmount } = setup({ clock: time.clock });
    await act(async () => {
      await result.current.signIn();
    });
    act(() => unmount());
    expect(time.cleared).toHaveLength(1);
    await time.fire();
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("falls back to the browser clock and the default interval", async () => {
    stubTransport({
      [START]: { status: 200, body: { authorize_url: "https://auth", state: "state-5" } },
      [STATUS]: { status: 200, body: { status: "ok" } },
    });
    const setInterval = vi.spyOn(globalThis, "setInterval");
    const clearInterval = vi.spyOn(globalThis, "clearInterval");
    const { result, unmount } = setup({});
    await act(async () => {
      await result.current.signIn();
    });
    expect(setInterval).toHaveBeenCalledWith(expect.any(Function), SIGN_IN_POLL_INTERVAL_MS);
    await act(async () => {
      unmount();
    });
    expect(clearInterval).toHaveBeenCalledTimes(1);
    setInterval.mockRestore();
    clearInterval.mockRestore();
  });

  it("stops an earlier poll when the user starts the flow again", async () => {
    const { answers } = stubTransport({
      [START]: { status: 200, body: { authorize_url: "https://auth", state: "state-6" } },
      [STATUS]: { status: 200, body: { status: "pending" } },
    });
    const time = manualClock();
    const { result } = setup({ clock: time.clock, intervalMs: 1_500 });
    await act(async () => {
      await result.current.signIn();
    });
    expect(time.callbacks).toHaveLength(1);
    answers[STATUS] = { status: 200, body: { status: "ok" } };
    await act(async () => {
      await result.current.signIn();
    });
    expect(time.cleared.length).toBeGreaterThanOrEqual(1);
    await act(async () => {
      result.current.signIn();
    });
    expect(time.cleared.length).toBeGreaterThanOrEqual(2);
  });
});
