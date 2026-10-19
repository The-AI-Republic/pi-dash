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
import { assistantKeys } from "../api/keys.js";
import { transcriptKey } from "../api/threadKeys.js";
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

interface SetupOptions {
  /** The thread list's `has_active_turn`. */
  active?: boolean;
  /** Hold each list read until the test resolves it. */
  holdListReads?: boolean;
}

function threadList(active: boolean) {
  return [
    {
      id: THREAD_ID,
      title: "Thread",
      is_archived: false,
      has_active_turn: active,
      created_at: "2026-10-01T00:00:00Z",
      updated_at: "2026-10-01T00:00:00Z",
    },
  ];
}

function setup(initial: AssistantMessage[], options: SetupOptions = {}) {
  let sends = 0;
  let reads = 0;
  let cancels = 0;
  let listReads = 0;
  // `active` is what the server answers about the running turn; tests flip
  // it to prove the settle path does not need a new transcript row.
  const server = { active: options.active ?? true };
  const held: Array<() => void> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    const path = new URL(url).pathname;
    if (path.endsWith("/ai-assistant/threads/") && init.method !== "POST") {
      listReads += 1;
      if (options.holdListReads === true) {
        const answer = JSON.stringify(threadList(server.active));
        await new Promise<void>((resolve) => held.push(resolve));
        return { status: 200, url, headers: { get: () => "application/json" }, text: async () => answer };
      }
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(threadList(server.active)),
      };
    }
    if (path.endsWith("/cancel/") && init.method === "POST") {
      cancels += 1;
      return { status: 204, url, headers: { get: () => null }, text: async () => "" };
    }
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
  return {
    ...hook,
    queryClient,
    server,
    sends: () => sends,
    reads: () => reads,
    cancels: () => cancels,
    listReads: () => listReads,
    /** Release every held list read, newest answer included. */
    releaseListReads: () => {
      for (const resolve of held.splice(0)) resolve();
    },
  };
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

  it("unlocks after stop when the cancel finalizes the turn without a new row", async () => {
    const { result, server, cancels } = setup([row()]);
    await waitFor(() => expect(result.current.running).toBe(true));
    act(() => result.current.stop());
    await waitFor(() => expect(cancels()).toBe(1));
    // The server ends the turn: no assistant or error row is ever added, so
    // only the thread list's `has_active_turn` can settle the composer.
    act(() => {
      server.active = false;
    });
    await waitFor(() => expect(result.current.running).toBe(false), { timeout: TURN_POLL_MS * 3 });
    expect(result.current.messages.map((message) => message.id)).toEqual(["u1"]);
  });

  it("stays locked after stop while the server still reports a running turn", async () => {
    const { result, cancels } = setup([row()]);
    await waitFor(() => expect(result.current.running).toBe(true));
    act(() => result.current.stop());
    await waitFor(() => expect(cancels()).toBe(1));
    await new Promise((resolve) => setTimeout(resolve, TURN_POLL_MS * 2));
    expect(result.current.running).toBe(true);
  });

  it("treats a cold-load trailing user row as running while the server says a turn is active", async () => {
    const { result, server } = setup([row()], { active: true });
    await waitFor(() => expect(result.current.loaded).toBe(true));
    await waitFor(() => expect(result.current.running).toBe(true));
    act(() => {
      server.active = false;
    });
    await waitFor(() => expect(result.current.running).toBe(false), { timeout: TURN_POLL_MS * 3 });
  });

  it("does not read an already-ended turn's tail user row as running once the server answers", async () => {
    // A cold load whose last user row belongs to a turn that has ended.
    const { result } = setup([row()], { active: false });
    await waitFor(() => expect(result.current.loaded).toBe(true));
    await waitFor(() => expect(result.current.running).toBe(false));
  });

  it("ignores a thread-list read that predates the turn it settles", async () => {
    const { result, queryClient } = setup([row({ id: "a", role: "assistant", seq: 2, content: "done" })], {
      active: true,
    });
    await waitFor(() => expect(result.current.loaded).toBe(true));
    // A stale list (older than the send) must not clear a turn that just started.
    act(() =>
      stream.subscriptions.at(-1)?.onEvent({
        id: 12,
        thread: THREAD_ID,
        message: null,
        seq: 12,
        kind: "turn_started",
        payload: { turn_id: "turn" },
        created_at: "2026-01-01T00:00:00Z",
      })
    );
    await waitFor(() => expect(result.current.running).toBe(true));
    act(() => {
      queryClient.setQueryData(assistantKeys.threads("acme"), threadList(false), { updatedAt: 1 });
    });
    expect(result.current.running).toBe(true);
  });

  it("does not settle a newer turn with a read that answered for an older one", async () => {
    const { result, releaseListReads } = setup([row({ id: "a", role: "assistant", seq: 2, content: "done" })], {
      active: false,
      holdListReads: true,
    });
    await waitFor(() => expect(result.current.loaded).toBe(true));
    act(() =>
      stream.subscriptions.at(-1)?.onEvent({
        id: 12,
        thread: THREAD_ID,
        message: null,
        seq: 12,
        kind: "turn_started",
        payload: { turn_id: "turn" },
        created_at: "2026-01-01T00:00:00Z",
      })
    );
    await waitFor(() => expect(result.current.running).toBe(true));
    act(() =>
      stream.subscriptions.at(-1)?.onEvent({
        id: 13,
        thread: THREAD_ID,
        message: null,
        seq: 13,
        kind: "turn_started",
        payload: { turn_id: "later" },
        created_at: "2026-01-01T00:00:01Z",
      })
    );
    // The read was asked for when the first turn started; the answer is
    // "idle", but it says nothing about the turn that started after it.
    act(() => releaseListReads());
    await new Promise((resolve) => setTimeout(resolve, TURN_POLL_MS));
    expect(result.current.running).toBe(true);
  });

  it("settles a sent turn from the list when no reply row ever arrives", async () => {
    const { result, server, sends, queryClient } = setup([]);
    await waitFor(() => expect(result.current.loaded).toBe(true));
    act(() => result.current.send("ask"));
    await waitFor(() => expect(sends()).toBe(1));
    act(() => {
      queryClient.setQueryData(transcriptKey("acme", THREAD_ID), [row({ id: "u2", content: "ask", seq: 2 })]);
    });
    await waitFor(() => expect(result.current.running).toBe(true));
    // The turn ends before any assistant or error row exists; only the
    // list's `has_active_turn` can say so.
    act(() => {
      server.active = false;
    });
    await waitFor(() => expect(result.current.running).toBe(false), { timeout: TURN_POLL_MS * 3 });
    expect(result.current.messages.map((message) => message.id)).toEqual(["u2"]);
  });

  it("ignores a list read that answers after the view is gone", async () => {
    // A cold load with a trailing user row starts a list read right away.
    const { result, releaseListReads, unmount } = setup([row()], { active: false, holdListReads: true });
    await waitFor(() => expect(result.current.loaded).toBe(true));
    await waitFor(() => expect(result.current.running).toBe(true));
    unmount();
    act(() => releaseListReads());
    await new Promise((resolve) => setTimeout(resolve, 0));
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
