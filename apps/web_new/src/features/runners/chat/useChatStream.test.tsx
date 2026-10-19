// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Live feed hook (NEWFRONT-551, RUN-029): frames reach the transcript once,
// terminal frames ask for a re-read, the strip derives from the feed, a
// transient error shows until the next frame or a dismissal, and a feed
// that ends reopens past the last sequence it saw.
import type { ChatEvent, ChatMessage } from "@pidash/api-client";
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ChatSubscriptionHandlers } from "./transport.js";
import { STREAM_RETRY_BASE_MS, STREAM_RETRY_MAX_MS, streamErrorMessage, useChatStream } from "./useChatStream.js";

const transportState = vi.hoisted(() => ({
  transport: null as unknown,
  handlers: [] as ChatSubscriptionHandlers[],
  subscriptions: [] as { sessionId: string; after: number }[],
  unsubscribes: [] as Array<ReturnType<typeof vi.fn>>,
}));

vi.mock("./transport.js", () => ({
  getChatTransport: () => transportState.transport,
}));

const STAMP = "2026-10-19T10:00:00Z";
let now = Date.parse(STAMP);

function frame(seq: number, overrides: Partial<ChatEvent> = {}): ChatEvent {
  return {
    id: seq,
    session: "s-1",
    message: "m-1",
    seq,
    kind: "assistant_delta",
    payload: { params: { delta: `d${seq}` } },
    created_at: STAMP,
    ...overrides,
  };
}

function message(overrides: Partial<ChatMessage> = {}): ChatMessage {
  return {
    id: "m-1",
    session: "s-1",
    role: "assistant",
    content: "base",
    content_parts: [],
    status: "streaming",
    local_item_id: "",
    local_turn_id: "",
    seq: 1,
    created_at: STAMP,
    completed_at: null,
    ...overrides,
  };
}

function install(handlers: ChatSubscriptionHandlers[] = []): void {
  const unsubscribe = vi.fn();
  transportState.handlers = [];
  transportState.subscriptions = [];
  transportState.unsubscribes = [unsubscribe];
  transportState.transport = {
    subscribeChatEvents(sessionId: string, after: number, next: ChatSubscriptionHandlers) {
      transportState.subscriptions.push({ sessionId, after });
      handlers.push(next);
      transportState.handlers.push(next);
      return unsubscribe;
    },
  };
}

function callbacks() {
  return { onHistoryRefresh: vi.fn(), onApprovalCleared: vi.fn() };
}

beforeEach(() => {
  now = Date.parse(STAMP);
  vi.spyOn(Date, "now").mockImplementation(() => now);
  install();
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("streamErrorMessage", () => {
  it("names only a failure that carries something readable", () => {
    expect(streamErrorMessage(new Error("broken"))).toBe("broken");
    expect(streamErrorMessage(new Error(""))).toBeNull();
    expect(streamErrorMessage("text")).toBe("text");
    expect(streamErrorMessage("")).toBeNull();
    expect(streamErrorMessage(undefined)).toBeNull();
    expect(streamErrorMessage({ code: 3 })).toBeNull();
  });
});

describe("useChatStream", () => {
  it("subscribes from the start for a session and stops on unmount", () => {
    const { unmount } = renderHook(({ id }) => useChatStream(id, [], callbacks()), { initialProps: { id: "s-1" } });
    expect(transportState.subscriptions).toEqual([{ sessionId: "s-1", after: 0 }]);
    unmount();
    expect(transportState.unsubscribes[0]).toHaveBeenCalledTimes(1);
  });

  it("does not subscribe without a session", () => {
    renderHook(() => useChatStream(null, [], callbacks()));
    expect(transportState.subscriptions).toEqual([]);
  });

  it("folds a live delta into the transcript exactly once", () => {
    const { result } = renderHook(() => useChatStream("s-1", [message()], callbacks()));
    act(() => transportState.handlers[0]?.onEvent?.(frame(1)));
    expect(result.current.messages[0]!.content).toBe("based1");
    act(() => transportState.handlers[0]?.onEvent?.(frame(1)));
    expect(result.current.messages[0]!.content).toBe("based1");
  });

  it("derives the activity strip from its frames", () => {
    const { result } = renderHook(() => useChatStream("s-1", [], callbacks()));
    act(() =>
      transportState.handlers[0]?.onEvent?.(
        frame(1, {
          kind: "raw",
          payload: { method: "item/completed", params: { item: { type: "commandExecution", command: "ls" } } },
        })
      )
    );
    expect(result.current.activity).toEqual([{ seq: 1, label: "Ran: ls" }]);
  });

  it("clears the error on the next frame", () => {
    const { result } = renderHook(() => useChatStream("s-1", [], callbacks()));
    act(() => transportState.handlers[0]?.onError?.(new Error("channel hiccup")));
    expect(result.current.error).toBe("channel hiccup");
    act(() => transportState.handlers[0]?.onEvent?.(frame(1, { payload: { params: {} } })));
    expect(result.current.error).toBeNull();
  });

  it("keeps a quiet channel failure silent but still refreshes", () => {
    const api = callbacks();
    const { result } = renderHook(() => useChatStream("s-1", [], api));
    act(() => transportState.handlers[0]?.onError?.(new Event("error")));
    expect(result.current.error).toBeNull();
    expect(api.onHistoryRefresh).toHaveBeenCalledTimes(1);
  });

  it("reports an unreadable frame and a real failure, refreshing each time", () => {
    const api = callbacks();
    const { result } = renderHook(() => useChatStream("s-1", [], api));
    act(() => transportState.handlers[0]?.onError?.(new Error("Chat frame did not match its contract")));
    expect(result.current.error).toBe("Chat frame did not match its contract");
    expect(api.onHistoryRefresh).toHaveBeenCalledTimes(1);
    act(() => result.current.dismissError());
    expect(result.current.error).toBeNull();
  });

  it("ignores a failure after the hook closed the feed", () => {
    const { result, unmount } = renderHook(() => useChatStream("s-1", [], callbacks()));
    const handler = transportState.handlers[0]!;
    unmount();
    act(() => handler.onError?.(new Error("late")));
    expect(result.current.error).toBeNull();
  });

  it("keeps a parked approval in the strip without clearing it", () => {
    const api = callbacks();
    const { result } = renderHook(() => useChatStream("s-1", [], api));
    act(() =>
      transportState.handlers[0]?.onEvent?.(
        frame(3, { kind: "chat_approval_request", message: null, payload: { params: {} } })
      )
    );
    expect(api.onApprovalCleared).not.toHaveBeenCalled();
    expect(api.onHistoryRefresh).not.toHaveBeenCalled();
    expect(result.current.activity).toHaveLength(1);
  });

  it("never opens over a subscription it already tore down", () => {
    vi.useFakeTimers();
    const { unmount } = renderHook(() => useChatStream("s-1", [], callbacks()));
    const handler = transportState.handlers[0]!;
    act(() => handler.onEnd?.());
    // The timer fires after the hook has closed: nothing reopens.
    unmount();
    vi.runOnlyPendingTimers();
    expect(transportState.subscriptions).toHaveLength(1);
  });

  it("clears the approval and refreshes on a terminal frame", () => {
    const api = callbacks();
    renderHook(() => useChatStream("s-1", [], api));
    act(() => transportState.handlers[0]?.onEvent?.(frame(2, { kind: "turn_completed", message: null, payload: {} })));
    expect(api.onApprovalCleared).toHaveBeenCalledTimes(1);
    expect(api.onHistoryRefresh).toHaveBeenCalledTimes(1);
  });

  it("does not clear the approval for a refresh-only frame", () => {
    const api = callbacks();
    renderHook(() => useChatStream("s-1", [], api));
    act(() => transportState.handlers[0]?.onEvent?.(frame(3, { kind: "turn_started", message: null, payload: {} })));
    expect(api.onApprovalCleared).not.toHaveBeenCalled();
    expect(api.onHistoryRefresh).toHaveBeenCalledTimes(1);
  });

  it("leaves approval and history alone for a frame that neither ends nor refreshes", () => {
    const api = callbacks();
    renderHook(() => useChatStream("s-1", [], api));
    act(() => transportState.handlers[0]?.onEvent?.(frame(4, { kind: "chat_timing", message: null, payload: {} })));
    expect(api.onApprovalCleared).not.toHaveBeenCalled();
    expect(api.onHistoryRefresh).not.toHaveBeenCalled();
  });

  it("reopens a feed that ended, resuming past the last sequence seen", async () => {
    vi.useFakeTimers();
    renderHook(() => useChatStream("s-1", [], callbacks()));
    const first = transportState.handlers[0]!;
    act(() => first.onEvent(frame(4)));
    transportState.handlers = [];
    act(() => first.onEnd?.());
    act(() => vi.advanceTimersByTime(STREAM_RETRY_BASE_MS));
    expect(transportState.subscriptions[1]).toEqual({ sessionId: "s-1", after: 4 });
    expect(transportState.handlers).toHaveLength(1);
  });

  it("backs off further with each ended feed and caps the wait", async () => {
    vi.useFakeTimers();
    renderHook(() => useChatStream("s-1", [], callbacks()));
    let handler = transportState.handlers[0]!;
    act(() => handler.onEnd?.());
    act(() => vi.advanceTimersByTime(STREAM_RETRY_BASE_MS));
    handler = transportState.handlers[transportState.handlers.length - 1]!;
    act(() => handler.onEnd?.());
    act(() => vi.advanceTimersByTime(STREAM_RETRY_BASE_MS));
    expect(transportState.subscriptions).toHaveLength(2);
    act(() => vi.advanceTimersByTime(STREAM_RETRY_BASE_MS));
    expect(transportState.subscriptions).toHaveLength(3);
  });

  it("resets the backoff after a frame arrives", async () => {
    vi.useFakeTimers();
    renderHook(() => useChatStream("s-1", [], callbacks()));
    const first = transportState.handlers[0]!;
    act(() => first.onEnd?.());
    act(() => vi.advanceTimersByTime(STREAM_RETRY_BASE_MS));
    const second = transportState.handlers[transportState.handlers.length - 1]!;
    act(() => second.onEvent(frame(7, { payload: { params: {} } })));
    act(() => second.onEnd?.());
    act(() => vi.advanceTimersByTime(STREAM_RETRY_BASE_MS));
    expect(transportState.subscriptions).toHaveLength(3);
  });

  it("closes a reopened feed that ended before its timer fired", async () => {
    vi.useFakeTimers();
    const { unmount } = renderHook(() => useChatStream("s-1", [], callbacks()));
    act(() => transportState.handlers[0]?.onEnd?.());
    // The component goes away while a reopen is pending: the timer is
    // cleared and the fresh subscription is never made.
    unmount();
    act(() => vi.advanceTimersByTime(STREAM_RETRY_BASE_MS * 2));
    expect(transportState.subscriptions).toHaveLength(1);
  });

  it("ignores a handler that fires after the hook moved on", () => {
    const api = callbacks();
    const { result, unmount } = renderHook(() => useChatStream("s-1", [], api));
    const handler = transportState.handlers[0]!;
    act(() => handler.onEnd?.());
    unmount();
    act(() => {
      handler.onError?.(new Error("late failure"));
      handler.onEvent?.(frame(9));
      handler.onEnd?.();
    });
    expect(result.current.messages).toEqual([]);
    expect(result.current.error).toBeNull();
    expect(api.onHistoryRefresh).not.toHaveBeenCalled();
  });

  it("waits at least once even when the timer is already late", () => {
    vi.useFakeTimers();
    renderHook(() => useChatStream("s-1", [], callbacks()));
    act(() => vi.advanceTimersByTime(STREAM_RETRY_BASE_MS * 4));
    act(() => transportState.handlers[0]?.onEnd?.());
    act(() => vi.advanceTimersByTime(STREAM_RETRY_BASE_MS - 1));
    expect(transportState.subscriptions).toHaveLength(1);
    act(() => vi.advanceTimersByTime(1));
    expect(transportState.subscriptions).toHaveLength(2);
  });

  it("caps the reopen wait after many dropped feeds", async () => {
    vi.useFakeTimers();
    renderHook(() => useChatStream("s-1", [], callbacks()));
    for (let attempt = 0; attempt < 6; attempt += 1) {
      const handler = transportState.handlers[transportState.handlers.length - 1]!;
      act(() => handler.onEnd?.());
      // Every wait so far is below the ceiling and fires the next reopen.
      act(() => vi.advanceTimersByTime(Math.min(STREAM_RETRY_MAX_MS, STREAM_RETRY_BASE_MS * 2 ** attempt)));
    }
    const count = transportState.subscriptions.length;
    act(() => transportState.handlers[transportState.handlers.length - 1]?.onEnd?.());
    act(() => vi.advanceTimersByTime(STREAM_RETRY_MAX_MS - 1));
    expect(transportState.subscriptions).toHaveLength(count);
    act(() => vi.advanceTimersByTime(1));
    expect(transportState.subscriptions).toHaveLength(count + 1);
  });

  it("does not reopen after unmount", async () => {
    vi.useFakeTimers();
    const { unmount } = renderHook(() => useChatStream("s-1", [], callbacks()));
    const handler = transportState.handlers[0]!;
    act(() => handler.onEnd?.());
    unmount();
    act(() => vi.advanceTimersByTime(STREAM_RETRY_BASE_MS * 4));
    expect(transportState.subscriptions).toHaveLength(1);
  });

  it("starts a fresh conversation when the session changes", async () => {
    vi.useRealTimers();
    const { result, rerender } = renderHook(({ id }) => useChatStream(id, [], callbacks()), {
      initialProps: { id: "s-1" as string | null },
    });
    act(() => transportState.handlers[0]?.onEvent?.(frame(5, { kind: "custom", message: null, payload: {} })));
    expect(result.current.activity).toHaveLength(1);
    const firstUnsubscribe = transportState.unsubscribes[0]!;
    rerender({ id: "s-2" });
    await waitFor(() => expect(transportState.subscriptions).toHaveLength(2));
    expect(firstUnsubscribe).toHaveBeenCalledTimes(1);
    expect(transportState.subscriptions[1]).toEqual({ sessionId: "s-2", after: 0 });
    await waitFor(() => expect(result.current.activity).toEqual([]));
    expect(result.current.error).toBeNull();
  });

  it("never hands the previous conversation's streamed rows to the new one", () => {
    const { result, rerender } = renderHook(({ id }) => useChatStream(id, [], callbacks()), {
      initialProps: { id: "s-1" as string | null },
    });
    act(() => transportState.handlers[0]?.onEvent?.(frame(1)));
    expect(result.current.messages).toHaveLength(1);
    // The render right after the switch happens before the reset effect;
    // the new conversation's read has not answered, so it reads empty.
    rerender({ id: "s-2" });
    expect(result.current.messages).toEqual([]);
  });

  it("layers the stream over a refreshed transcript without reopening the feed", async () => {
    const { result, rerender } = renderHook(
      ({ base }: { base: ChatMessage[] }) => useChatStream("s-1", base, callbacks()),
      { initialProps: { base: [message()] } }
    );
    act(() => transportState.handlers[0]?.onEvent?.(frame(1)));
    expect(result.current.messages[0]!.content).toBe("based1");
    rerender({ base: [message({ content: "saved" })] });
    await waitFor(() => expect(result.current.messages[0]!.content).toBe("saved"));
    expect(transportState.subscriptions).toHaveLength(1);
  });

  it("drops a replayed frame that predates the subscription", () => {
    now = Date.parse("2026-10-19T10:00:00Z");
    const { result } = renderHook(() => useChatStream("s-1", [], callbacks()));
    act(() => transportState.handlers[0]?.onEvent?.(frame(1, { created_at: "2026-10-19T08:00:00Z" })));
    expect(result.current.messages).toEqual([]);
  });

  it("uses the latest callbacks without resubscribing", () => {
    const first = callbacks();
    const second = callbacks();
    const { rerender } = renderHook(({ api }: { api: ReturnType<typeof callbacks> }) => useChatStream("s-1", [], api), {
      initialProps: { api: first },
    });
    rerender({ api: second });
    act(() => transportState.handlers[0]?.onEvent?.(frame(6, { kind: "turn_completed", message: null, payload: {} })));
    expect(first.onHistoryRefresh).not.toHaveBeenCalled();
    expect(second.onHistoryRefresh).toHaveBeenCalledTimes(1);
    expect(transportState.subscriptions).toHaveLength(1);
  });
});
