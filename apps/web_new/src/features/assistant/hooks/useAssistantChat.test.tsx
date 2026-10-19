// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Live chat state (AGT-042): the stored page paints the transcript, a
// streaming row or trailing user row turns the poll on, a terminal reply
// settles it, and a blank send never reaches the server.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { AssistantMessage } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { transcriptKey } from "../api/keys.js";
import { TURN_POLL_MS, useAssistantChat } from "./useAssistantChat.js";

const stream = vi.hoisted(() => ({ subscriptions: [] as { onEvent: (event: unknown) => void }[] }));

vi.mock("../api/events.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../api/events.js")>();
  return {
    ...actual,
    subscribeAssistantEvents: (
      _slug: string,
      _thread: string,
      _after: number,
      handlers: { onEvent: (event: unknown) => void }
    ) => {
      stream.subscriptions.push(handlers);
      return { close: vi.fn() };
    },
  };
});

const THREAD_ID = "11111111-1111-4111-8111-111111111111";

function row(overrides: Partial<AssistantMessage> = {}): AssistantMessage {
  return {
    id: "u1",
    role: "user",
    content: "ask",
    status: "completed",
    seq: 1,
    turn_id: null,
    payload: {},
    created_at: "2026-10-01T00:00:00Z",
    completed_at: "2026-10-01T00:00:00Z",
    ...overrides,
  };
}

function setup(initial: AssistantMessage[]) {
  let sends = 0;
  let reads = 0;
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    const path = new URL(url).pathname;
    if (path.endsWith("/messages/") && init.method === "POST") {
      sends += 1;
      return {
        status: 202,
        url,
        headers: { get: () => "application/json" },
        text: async () =>
          JSON.stringify({
            turn: { id: "33333333-3333-4333-8333-333333333333", status: "queued" },
            message: row({ id: "u2", content: "next", seq: 2 }),
          }),
      };
    }
    if (path.endsWith("/messages/")) {
      reads += 1;
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(initial),
      };
    }
    return { status: 500, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  queryClient.setQueryData(transcriptKey("acme", THREAD_ID), initial);
  const hook = renderHook(() => useAssistantChat("acme", THREAD_ID), {
    wrapper: ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>
        <React.Fragment>{children}</React.Fragment>
      </QueryClientProvider>
    ),
  });
  return { ...hook, queryClient, sends: () => sends, reads: () => reads };
}

afterEach(() => {
  resetAppClient();
  stream.subscriptions.length = 0;
});

describe("useAssistantChat", () => {
  it("exposes the stored rows oldest first and is not running", async () => {
    const { result } = setup([
      row({ id: "a", seq: 2, role: "assistant", content: "answer" }),
      row({ id: "u", seq: 1 }),
    ]);
    await waitFor(() => expect(result.current.loaded).toBe(true));
    expect(result.current.messages.map((message) => message.id)).toEqual(["u", "a"]);
    expect(result.current.running).toBe(false);
  });

  it("treats a trailing user row or streaming row as a running turn", async () => {
    const userLast = setup([row()]);
    await waitFor(() => expect(userLast.result.current.loaded).toBe(true));
    expect(userLast.result.current.running).toBe(true);
    userLast.unmount();
    const streaming = setup([row({ id: "a", role: "assistant", status: "streaming", seq: 2 })]);
    await waitFor(() => expect(streaming.result.current.loaded).toBe(true));
    expect(streaming.result.current.running).toBe(true);
  });

  it("settles on a terminal turn-completed event even before the page catches up", async () => {
    const { result } = setup([row()]);
    await waitFor(() => expect(result.current.running).toBe(true));
    stream.subscriptions.at(-1)?.onEvent({
      id: 5,
      thread: THREAD_ID,
      message: null,
      seq: 5,
      kind: "turn_completed",
      payload: { turn_id: "turn", usage: {} },
      created_at: "2026-01-01T00:00:00Z",
    });
    await waitFor(() => expect(result.current.running).toBe(false));
  });

  it("marks a turn running from a turn-started event", async () => {
    const { result } = setup([row({ id: "a", role: "assistant", seq: 2, content: "done" })]);
    await waitFor(() => expect(result.current.loaded).toBe(true));
    expect(result.current.running).toBe(false);
    act(() =>
      stream.subscriptions.at(-1)?.onEvent({
        id: 6,
        thread: THREAD_ID,
        message: null,
        seq: 6,
        kind: "turn_started",
        payload: { turn_id: "turn" },
        created_at: "2026-01-01T00:00:00Z",
      })
    );
    await waitFor(() => expect(result.current.running).toBe(true));
    act(() =>
      stream.subscriptions.at(-1)?.onEvent({
        id: 7,
        thread: THREAD_ID,
        message: null,
        seq: 7,
        kind: "message_completed",
        payload: { message: row({ id: "b", role: "assistant", seq: 3, content: "answer" }) },
        created_at: "2026-01-01T00:00:00Z",
      })
    );
    await waitFor(() => expect(result.current.messages.map((message) => message.id)).toContain("b"));
    await waitFor(() => expect(result.current.running).toBe(false));
  });

  it("ignores an event kind the view does not know", async () => {
    const { result } = setup([row({ id: "a", role: "assistant", seq: 2, content: "done" })]);
    await waitFor(() => expect(result.current.loaded).toBe(true));
    act(() =>
      stream.subscriptions.at(-1)?.onEvent({
        id: 8,
        thread: THREAD_ID,
        message: null,
        seq: 8,
        kind: "future_kind",
        payload: { x: 1 },
        created_at: "2026-01-01T00:00:00Z",
      })
    );
    expect(result.current.running).toBe(false);
    expect(result.current.messages.map((message) => message.id)).toEqual(["a"]);
  });

  it("settles from the poll when a terminal reply arrives without an event", async () => {
    const { result, queryClient } = setup([row({ id: "u", seq: 1 })]);
    await waitFor(() => expect(result.current.running).toBe(true));
    act(() =>
      queryClient.setQueryData(transcriptKey("acme", THREAD_ID), [
        row({ id: "u", seq: 1 }),
        row({ id: "a", role: "assistant", seq: 2, content: "answer" }),
      ])
    );
    await waitFor(() => expect(result.current.running).toBe(false));
  });

  it("clears a started turn when the poll lands its terminal reply", async () => {
    const { result, queryClient } = setup([row({ id: "u", seq: 1 })]);
    await waitFor(() => expect(result.current.loaded).toBe(true));
    act(() =>
      stream.subscriptions.at(-1)?.onEvent({
        id: 11,
        thread: THREAD_ID,
        message: null,
        seq: 11,
        kind: "turn_started",
        payload: { turn_id: "turn" },
        created_at: "2026-01-01T00:00:00Z",
      })
    );
    expect(result.current.running).toBe(true);
    act(() =>
      queryClient.setQueryData(transcriptKey("acme", THREAD_ID), [
        row({ id: "u", seq: 1 }),
        row({ id: "a", role: "assistant", seq: 2, content: "answer" }),
      ])
    );
    await waitFor(() => expect(result.current.running).toBe(false));
  });

  it("does not settle on a reply that predates the turn", async () => {
    const { result, queryClient } = setup([row({ id: "u", seq: 1 })]);
    await waitFor(() => expect(result.current.running).toBe(true));
    act(() =>
      stream.subscriptions.at(-1)?.onEvent({
        id: 9,
        thread: THREAD_ID,
        message: null,
        seq: 9,
        kind: "turn_started",
        payload: { turn_id: "turn" },
        created_at: "2026-01-01T00:00:00Z",
      })
    );
    // A stale page lands an older completed reply; this turn is still going.
    act(() =>
      queryClient.setQueryData(transcriptKey("acme", THREAD_ID), [
        row({ id: "a", role: "assistant", seq: 1, content: "old answer" }),
      ])
    );
    await waitFor(() => expect(result.current.messages.at(-1)?.id).toBe("a"));
    expect(result.current.running).toBe(true);
  });

  it("does not send blank content", async () => {
    const { result, sends } = setup([row({ id: "a", role: "assistant", seq: 2, content: "done" })]);
    await waitFor(() => expect(result.current.loaded).toBe(true));
    result.current.send("   ");
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(sends()).toBe(0);
  });

  it("polls the transcript every second while a turn is in flight", async () => {
    const running = setup([row()]);
    await waitFor(() => expect(running.result.current.running).toBe(true));
    const before = running.reads();
    await new Promise((resolve) => setTimeout(resolve, TURN_POLL_MS + 400));
    expect(running.reads()).toBeGreaterThan(before);
    running.unmount();

    const settled = setup([row({ id: "a", role: "assistant", seq: 2, content: "done" })]);
    await waitFor(() => expect(settled.result.current.loaded).toBe(true));
    const steady = settled.reads();
    await new Promise((resolve) => setTimeout(resolve, TURN_POLL_MS + 400));
    expect(settled.reads()).toBe(steady);
  });
});
