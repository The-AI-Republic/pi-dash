// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Chat event transport (NEWFRONT-549, RUN-032): the web default opens the
// session's SSE feed through the platform stream, parses `chat.event`
// frames against the contract, and reports unreadable frames and broken
// channels without duplicating frames.
import type { ApiClient, ChatEvent } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { platform, type EventStream, type StreamInit, type StreamMessage } from "../../../core/platform/index.js";
import { createCloudChatTransport, getChatTransport, setChatTransport } from "./transport.js";

const client = { baseUrl: "https://api.test" } as ApiClient;

function event(seq: number, overrides: Partial<ChatEvent> = {}): ChatEvent {
  return {
    id: seq,
    session: "11111111-1111-4111-8111-111111111111",
    message: null,
    seq,
    kind: "chat_timing",
    payload: {},
    created_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

interface Harness {
  feed: { close: ReturnType<typeof vi.fn>; messages: () => AsyncIterable<StreamMessage> };
  opened: Array<{ url: string; init: StreamInit }>;
}

/** A stream whose messages are pushed by the test. */
function harness(messages: StreamMessage[] | (() => AsyncIterable<StreamMessage>)): Harness {
  const opened: Array<{ url: string; init: StreamInit }> = [];
  const gate = new Promise<void>(() => undefined);
  const feed = {
    close: vi.fn(),
    messages: async function* (): AsyncIterable<StreamMessage> {
      if (typeof messages === "function") {
        yield* messages();
        return;
      }
      for (const message of messages) yield message;
      await gate;
    },
  };
  return { feed, opened };
}

function transportWith(harnessValue: Harness): { transport: ReturnType<typeof createCloudChatTransport> } {
  const transport = createCloudChatTransport(
    () => client,
    (url, init) => {
      harnessValue.opened.push({ url, init });
      return harnessValue.feed as unknown as EventStream;
    }
  );
  return { transport };
}

afterEach(() => {
  setChatTransport(null);
});

describe("createCloudChatTransport", () => {
  it("opens the session feed with a resume cursor and reads frames in order", async () => {
    const frames = [event(4), event(5, { kind: "assistant_delta" })];
    const h = harness(frames.map((frame) => ({ event: "chat.event", data: JSON.stringify(frame) })));
    const { transport } = transportWith(h);
    const onEvent = vi.fn();
    transport.subscribeChatEvents("s-1", 3, { onEvent });
    await vi.waitFor(() => expect(onEvent).toHaveBeenCalledTimes(2));
    expect(h.opened[0]?.url).toBe("https://api.test/api/runners/chat/sessions/s-1/events/?after=3");
    expect(h.opened[0]?.init).toEqual({ headers: { Accept: "text/event-stream" } });
    expect(onEvent.mock.calls.map((call) => (call[0] as ChatEvent).seq)).toEqual([4, 5]);
  });

  it("starts from the bare feed without a cursor when after is zero", async () => {
    const h = harness([]);
    const { transport } = transportWith(h);
    transport.subscribeChatEvents("s-2", 0, { onEvent: vi.fn() });
    expect(h.opened[0]?.url).toBe("https://api.test/api/runners/chat/sessions/s-2/events/");
  });

  it("ignores heartbeat frames and reports unreadable or off-contract frames", async () => {
    const h = harness([
      { event: "message", data: "" },
      { event: "chat.event", data: "not json" },
      { event: "chat.event", data: JSON.stringify({ kind: "chat_timing" }) },
      { event: "chat.event", data: JSON.stringify(event(9)) },
    ]);
    const { transport } = transportWith(h);
    const onEvent = vi.fn();
    const onError = vi.fn();
    transport.subscribeChatEvents("s-3", 0, { onEvent, onError });
    await vi.waitFor(() => expect(onEvent).toHaveBeenCalledTimes(1));
    expect(onError).toHaveBeenCalledTimes(2);
    expect(onError.mock.calls[0]?.[0]).toBeInstanceOf(Error);
  });

  it("reports a failing channel once and nothing after unsubscribe", async () => {
    const h = harness(async function* (): AsyncIterable<StreamMessage> {
      await new Promise((resolve) => setTimeout(resolve, 5));
      yield { event: "message", data: "" };
      throw new Error("channel dropped");
    });
    const { transport } = transportWith(h);
    const onError = vi.fn();
    transport.subscribeChatEvents("s-4", 0, { onEvent: vi.fn(), onError });
    await vi.waitFor(() => expect(onError).toHaveBeenCalledWith(new Error("channel dropped")));
  });

  it("closes the feed on unsubscribe and is idempotent", () => {
    const h = harness([]);
    const { transport } = transportWith(h);
    const unsubscribe = transport.subscribeChatEvents("s-5", 0, { onEvent: vi.fn() });
    unsubscribe();
    unsubscribe();
    expect(h.feed.close).toHaveBeenCalledTimes(1);
  });

  it("works without an error handler", async () => {
    const h = harness([{ event: "chat.event", data: "broken" }]);
    const { transport } = transportWith(h);
    const onEvent = vi.fn();
    transport.subscribeChatEvents("s-6", 0, { onEvent });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(onEvent).not.toHaveBeenCalled();
  });
});

describe("active transport", () => {
  it("defaults to the cloud transport and restores it when cleared", () => {
    const initial = getChatTransport();
    const custom = { subscribeChatEvents: vi.fn(() => () => undefined) };
    setChatTransport(custom);
    expect(getChatTransport()).toBe(custom);
    setChatTransport(null);
    expect(getChatTransport()).toBe(initial);
    setChatTransport(undefined);
    expect(getChatTransport()).toBe(initial);
  });
});

describe("createCloudChatTransport resilience", () => {
  it("reports a message that cannot be read as a channel failure", async () => {
    const unreadable = { event: "chat.event" } as unknown as StreamMessage;
    Object.defineProperty(unreadable, "data", {
      get() {
        throw new Error("stream read failed");
      },
    });
    const h = harness([unreadable]);
    const { transport } = transportWith(h);
    const onEvent = vi.fn();
    const onError = vi.fn();
    transport.subscribeChatEvents("s-7", 0, { onEvent, onError });
    await vi.waitFor(() => expect(onError).toHaveBeenCalledWith(new Error("stream read failed")));
    expect(onEvent).not.toHaveBeenCalled();
  });

  it("stops reading and does not report a channel error after unsubscribe", async () => {
    let release: () => void = () => undefined;
    const h = harness(async function* (): AsyncIterable<StreamMessage> {
      await new Promise<void>((resolve) => (release = resolve));
      yield { event: "message", data: "" };
      throw new Error("late failure");
    });
    const { transport } = transportWith(h);
    const onError = vi.fn();
    const onEvent = vi.fn();
    const unsubscribe = transport.subscribeChatEvents("s-8", 0, { onEvent, onError });
    unsubscribe();
    release();
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(onError).not.toHaveBeenCalled();
    expect(onEvent).not.toHaveBeenCalled();
  });

  it("ignores frames that arrive after the feed was closed", async () => {
    let release: () => void = () => undefined;
    const h = harness(async function* (): AsyncIterable<StreamMessage> {
      await new Promise<void>((resolve) => (release = resolve));
      yield { event: "chat.event", data: JSON.stringify(event(12)) };
    });
    const { transport } = transportWith(h);
    const onEvent = vi.fn();
    transport.subscribeChatEvents("s-9", 0, { onEvent })();
    release();
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(onEvent).not.toHaveBeenCalled();
  });
  it("finishes quietly when the channel closes on its own", async () => {
    const h = harness(async function* (): AsyncIterable<StreamMessage> {
      yield { event: "chat.event", data: JSON.stringify(event(15)) };
    });
    const { transport } = transportWith(h);
    const onEvent = vi.fn();
    const onError = vi.fn();
    transport.subscribeChatEvents("s-10", 0, { onEvent, onError });
    await vi.waitFor(() => expect(onEvent).toHaveBeenCalledTimes(1));
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(onError).not.toHaveBeenCalled();
  });
  it("opens through the platform stream by default", async () => {
    const stream = vi.spyOn(platform, "stream").mockReturnValue({
      close: vi.fn(),
      messages: async function* (): AsyncIterable<StreamMessage> {
        yield { event: "chat.event", data: JSON.stringify(event(21)) };
      },
    });
    const transport = createCloudChatTransport(() => client);
    const onEvent = vi.fn();
    transport.subscribeChatEvents("s-11", 0, { onEvent });
    await vi.waitFor(() => expect(onEvent).toHaveBeenCalledTimes(1));
    expect(stream).toHaveBeenCalledWith("https://api.test/api/runners/chat/sessions/s-11/events/", {
      headers: { Accept: "text/event-stream" },
    });
    stream.mockRestore();
  });
});
