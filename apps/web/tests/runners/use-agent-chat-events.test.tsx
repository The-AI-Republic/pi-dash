/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { IAgentChatEvent } from "@pi-dash/types";

const { getChatTransport } = vi.hoisted(() => ({
  getChatTransport: vi.fn(),
}));

vi.mock("@pi-dash/services", () => ({
  getChatTransport,
}));

import { useAgentChatEvents } from "../../core/components/runners/chat/use-agent-chat-events";

type SubscribeArgs = {
  sessionId: string;
  after: number;
  onEvent: (event: IAgentChatEvent) => void;
  onError?: (error: unknown) => void;
};

function installTransport() {
  const unsubscribe = vi.fn();
  let captured: SubscribeArgs | undefined;
  const subscribeChatEvents = vi.fn(
    (sessionId: string, after: number, onEvent: SubscribeArgs["onEvent"], onError?: SubscribeArgs["onError"]) => {
      captured = { sessionId, after, onEvent, onError };
      return unsubscribe;
    }
  );
  getChatTransport.mockReturnValue({ subscribeChatEvents });
  return {
    subscribeChatEvents,
    unsubscribe,
    get captured() {
      return captured;
    },
  };
}

const EVENT: IAgentChatEvent = {
  id: 1,
  session: "session-1",
  message: "msg-1",
  seq: 7,
  kind: "assistant_delta",
  payload: {},
  created_at: "2026-09-14T00:00:00Z",
};

afterEach(() => {
  vi.clearAllMocks();
});

describe("useAgentChatEvents", () => {
  it("subscribes through the active chat transport for a session", () => {
    const t = installTransport();
    const onEvent = vi.fn();
    renderHook(() => useAgentChatEvents("session-1", onEvent, undefined, 3));

    expect(t.subscribeChatEvents).toHaveBeenCalledTimes(1);
    expect(t.captured?.sessionId).toBe("session-1");
    expect(t.captured?.after).toBe(3);
  });

  it("does not subscribe when there is no session id", () => {
    const t = installTransport();
    renderHook(() => useAgentChatEvents(undefined, vi.fn()));
    expect(t.subscribeChatEvents).not.toHaveBeenCalled();
  });

  it("delivers stream events to the latest onEvent callback", () => {
    const t = installTransport();
    const onEvent = vi.fn();
    const { rerender } = renderHook(({ cb }) => useAgentChatEvents("session-1", cb), {
      initialProps: { cb: onEvent },
    });

    // Swap the callback without changing the session: the hook must not
    // re-subscribe, and must invoke the *new* callback (refs, not deps).
    const onEvent2 = vi.fn();
    rerender({ cb: onEvent2 });
    expect(t.subscribeChatEvents).toHaveBeenCalledTimes(1);

    t.captured?.onEvent(EVENT);
    expect(onEvent).not.toHaveBeenCalled();
    expect(onEvent2).toHaveBeenCalledWith(EVENT);
  });

  it("routes stream errors to the latest onError callback", () => {
    const t = installTransport();
    const onError = vi.fn();
    renderHook(() => useAgentChatEvents("session-1", vi.fn(), onError));

    const boom = new Error("stream failed");
    t.captured?.onError?.(boom);
    expect(onError).toHaveBeenCalledWith(boom);
  });

  it("unsubscribes on unmount", () => {
    const t = installTransport();
    const { unmount } = renderHook(() => useAgentChatEvents("session-1", vi.fn()));
    expect(t.unsubscribe).not.toHaveBeenCalled();
    unmount();
    expect(t.unsubscribe).toHaveBeenCalledTimes(1);
  });

  it("resubscribes when the session id changes, tearing down the old stream", () => {
    const t = installTransport();
    const { rerender } = renderHook(({ id }) => useAgentChatEvents(id, vi.fn()), {
      initialProps: { id: "session-1" },
    });
    rerender({ id: "session-2" });

    expect(t.unsubscribe).toHaveBeenCalledTimes(1);
    expect(t.subscribeChatEvents).toHaveBeenCalledTimes(2);
    expect(t.captured?.sessionId).toBe("session-2");
  });
});
