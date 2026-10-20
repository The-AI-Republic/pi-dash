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
import {
  chatTransportForRunner,
  createCloudChatTransport,
  fetchChatMessages,
  fetchChatRunner,
  fetchChatSessions,
  getChatTransport,
  openChatSession,
  setChatTransport,
  subscribeChatTransport,
  type ChatTransport,
} from "./transport.js";

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
  feed: { close: ReturnType<typeof vi.fn>; messages: () => AsyncIterable<StreamMessage>; error: undefined };
  opened: Array<{ url: string; init: StreamInit }>;
}

/** A stream whose messages are pushed by the test. */
function harness(messages: StreamMessage[] | (() => AsyncIterable<StreamMessage>)): Harness {
  const opened: Array<{ url: string; init: StreamInit }> = [];
  const gate = new Promise<void>(() => undefined);
  const feed = {
    close: vi.fn(),
    error: undefined,
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
  const transport = createCloudChatTransport({
    getClient: () => client,
    openStream: (url, init) => {
      harnessValue.opened.push({ url, init });
      return harnessValue.feed as unknown as EventStream;
    },
  });
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

  it("stays quiet when the channel fails after it was closed", async () => {
    const h = harness(async function* (): AsyncIterable<StreamMessage> {
      await new Promise((resolve) => setTimeout(resolve, 5));
      throw new Error("late");
      yield { event: "chat.event", data: "" };
    });
    const { transport } = transportWith(h);
    const onError = vi.fn();
    const onEnd = vi.fn();
    const unsubscribe = transport.subscribeChatEvents("s-8", 0, { onEvent: vi.fn(), onError, onEnd });
    unsubscribe();
    await new Promise((resolve) => setTimeout(resolve, 15));
    expect(onError).not.toHaveBeenCalled();
    expect(onEnd).not.toHaveBeenCalled();
  });

  it("reports a non-Event rejection and still signals the feed ended", async () => {
    const h = harness(async function* (): AsyncIterable<StreamMessage> {
      yield { event: "message", data: "" };
      throw new TypeError("socket reset");
    });
    const { transport } = transportWith(h);
    const onError = vi.fn();
    const onEnd = vi.fn();
    transport.subscribeChatEvents("s-7", 0, { onEvent: vi.fn(), onError, onEnd });
    await vi.waitFor(() => expect(onEnd).toHaveBeenCalledTimes(1));
    expect(onError).toHaveBeenCalledWith(expect.any(TypeError));
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
    const custom = { subscribeChatEvents: vi.fn(() => () => undefined) } as unknown as ChatTransport;
    setChatTransport(custom);
    expect(getChatTransport()).toBe(custom);
    setChatTransport(null);
    expect(getChatTransport()).toBe(initial);
    setChatTransport(undefined);
    expect(getChatTransport()).toBe(initial);
  });
});

describe("read and write seams", () => {
  it("opens a conversation through the API when no transport replaces it", async () => {
    const send = vi.fn(async () => ({ data: { id: "s-new" } }));
    const parse = vi.fn(() => ({ id: "s-new" }));
    const api = { sendJson: send, parse } as unknown as ApiClient;
    await expect(openChatSession(api, { workspace: "ws-id", runner: "r-1" })).resolves.toEqual({ id: "s-new" });
    expect(send).toHaveBeenCalledWith("POST", "/api/runners/chat/sessions/", {
      json: { workspace: "ws-id", runner: "r-1" },
    });
  });

  it("opens a conversation through the active transport when it has the verb", async () => {
    const createChatSession = vi.fn(async () => ({ id: "local" }));
    setChatTransport({
      subscribeChatEvents: vi.fn(() => () => undefined),
      createChatSession,
    } as unknown as ChatTransport);
    await expect(openChatSession({} as ApiClient, { workspace: "ws-id", runner: "r-1" })).resolves.toEqual({
      id: "local",
    });
    expect(createChatSession).toHaveBeenCalledWith({ workspace: "ws-id", runner: "r-1" });
  });

  it("falls through to the API for everything the local transport declines", async () => {
    const sendJson = vi.fn(async () => ({ data: { id: "s-new" } }));
    const getJson = vi.fn(async () => []);
    const parse = vi.fn((_schema: unknown, body: unknown) => body);
    const api = { sendJson, getJson, parse } as unknown as ApiClient;
    setChatTransport({
      subscribeChatEvents: vi.fn(() => () => undefined),
      chatRunner: vi.fn(async () => null),
      listChatSessions: vi.fn(async () => null),
      createChatSession: vi.fn(async () => null),
      listChatMessages: vi.fn(async () => null),
      ownsChatSession: vi.fn(async () => false),
    } as unknown as ChatTransport);
    await expect(openChatSession(api, { workspace: "ws-id", runner: "r-1" })).resolves.toEqual({ id: "s-new" });
    expect(sendJson).toHaveBeenCalledWith("POST", "/api/runners/chat/sessions/", {
      json: { workspace: "ws-id", runner: "r-1" },
    });
    await expect(fetchChatSessions(api, "ws-id", "r-1")).resolves.toEqual([]);
    await expect(fetchChatMessages(api, "s-1")).resolves.toEqual([]);
    expect(getJson).toHaveBeenCalledWith("/api/runners/chat/sessions/s-1/messages/");
  });

  it("reads a server runner's detail even while a local transport is installed", async () => {
    const getJson = vi.fn(async () => ({ id: "r-1" }));
    const parse = vi.fn(() => ({ id: "r-1" }));
    const api = { getJson, parse } as unknown as ApiClient;
    setChatTransport({
      subscribeChatEvents: vi.fn(() => () => undefined),
      chatRunner: vi.fn(async (runnerId: string) => (runnerId === "pidash-builtin" ? { id: runnerId } : null)),
    } as unknown as ChatTransport);
    await expect(fetchChatRunner(api, "pidash-builtin")).resolves.toEqual({ id: "pidash-builtin" });
    await expect(fetchChatRunner(api, "r-1")).resolves.toEqual({ id: "r-1" });
    expect(getJson).toHaveBeenCalledWith("/api/runners/r-1/");
  });

  it("reads a transcript from the local store only when the transport owns it", async () => {
    const listChatMessages = vi.fn(async () => [{ id: "m-local" }]);
    const ownsChatSession = vi.fn(async (sessionId: string) => sessionId === "local");
    setChatTransport({
      subscribeChatEvents: vi.fn(() => () => undefined),
      listChatMessages,
      ownsChatSession,
    } as unknown as ChatTransport);
    const api = { getJson: vi.fn(async () => []), parse: vi.fn(() => []) } as unknown as ApiClient;
    await expect(fetchChatMessages(api, "local")).resolves.toEqual([{ id: "m-local" }]);
    await expect(fetchChatMessages(api, "server")).resolves.toEqual([]);
    expect(ownsChatSession).toHaveBeenCalledWith("server");
  });

  it("keeps a server contact on the cloud surface while the local transport is installed", () => {
    const local = {
      subscribeChatEvents: vi.fn(() => () => undefined),
      ownsRunner: (runnerId: string) => runnerId === "pidash-builtin",
    } as unknown as ChatTransport;
    setChatTransport(local);
    expect(chatTransportForRunner("pidash-builtin")).toBe(local);
    expect(chatTransportForRunner("r-1")).not.toBe(local);
    // A transport that names no contacts is the whole surface.
    const whole = { subscribeChatEvents: vi.fn(() => () => undefined) } as unknown as ChatTransport;
    setChatTransport(whole);
    expect(chatTransportForRunner("r-1")).toBe(whole);
  });

  it("notifies subscribers when the transport is replaced or restored", () => {
    const listener = vi.fn();
    const stop = subscribeChatTransport(listener);
    const custom = { subscribeChatEvents: vi.fn(() => () => undefined) } as unknown as ChatTransport;
    setChatTransport(custom);
    expect(listener).toHaveBeenCalledTimes(1);
    setChatTransport(null);
    expect(listener).toHaveBeenCalledTimes(2);
    stop();
    setChatTransport(custom);
    expect(listener).toHaveBeenCalledTimes(2);
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
  it("does not reopen a channel that ends after unsubscribe", async () => {
    let started: () => void = () => undefined;
    let close: () => void = () => undefined;
    const begun = new Promise<void>((resolve) => (started = resolve));
    const h = harness(async function* (): AsyncIterable<StreamMessage> {
      started();
      yield { event: "chat.event", data: JSON.stringify(event(16)) };
      await new Promise<void>((resolve) => (close = resolve));
      return;
    });
    const { transport } = transportWith(h);
    const onEvent = vi.fn();
    const onEnd = vi.fn();
    const unsubscribe = transport.subscribeChatEvents("s-9", 0, { onEvent, onEnd });
    await begun;
    // Let the drain take the first frame and park on the generator before
    // the unsubscribe, so the channel really ends after it was closed.
    await new Promise((resolve) => setTimeout(resolve, 5));
    expect(onEvent).toHaveBeenCalledTimes(1);
    unsubscribe();
    close();
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(onEvent).toHaveBeenCalledTimes(1);
    expect(onEnd).not.toHaveBeenCalled();
  });

  it("ends a channel that has no end handler", async () => {
    const h = harness(async function* (): AsyncIterable<StreamMessage> {
      yield { event: "chat.event", data: JSON.stringify(event(17)) };
    });
    const { transport } = transportWith(h);
    const onEvent = vi.fn();
    transport.subscribeChatEvents("s-10", 0, { onEvent });
    await vi.waitFor(() => expect(onEvent).toHaveBeenCalledTimes(1));
    await new Promise((resolve) => setTimeout(resolve, 10));
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
      error: undefined,
      messages: async function* (): AsyncIterable<StreamMessage> {
        yield { event: "chat.event", data: JSON.stringify(event(21)) };
      },
    });
    const transport = createCloudChatTransport({ getClient: () => client });
    const onEvent = vi.fn();
    transport.subscribeChatEvents("s-11", 0, { onEvent });
    await vi.waitFor(() => expect(onEvent).toHaveBeenCalledTimes(1));
    expect(stream).toHaveBeenCalledWith("https://api.test/api/runners/chat/sessions/s-11/events/", {
      headers: { Accept: "text/event-stream" },
    });
    stream.mockRestore();
  });

  it("sends, cancels and closes through the session's own verbs", async () => {
    const calls: Array<{ method: string; path: string; json?: unknown }> = [];
    const rows = {
      sendJson: async (method: string, path: string, init: { json?: unknown }) => {
        calls.push({ method, path, json: init.json });
        return { data: {} };
      },
      parse: (_schema: unknown, _body: unknown, label: string) => ({ label }),
    } as unknown as ApiClient;
    const transport = createCloudChatTransport({ getClient: () => rows });
    await expect(transport.sendChatMessage("s-20", "hello")).resolves.toEqual({
      label: "POST /api/runners/chat/sessions/s-20/messages/",
    });
    await transport.cancelChat("s-20", "user_switched_model");
    await transport.closeChat("s-20");
    expect(calls).toEqual([
      {
        method: "POST",
        path: "/api/runners/chat/sessions/s-20/messages/",
        json: { content: "hello", content_parts: [] },
      },
      { method: "POST", path: "/api/runners/chat/sessions/s-20/cancel/", json: { reason: "user_switched_model" } },
      { method: "POST", path: "/api/runners/chat/sessions/s-20/close/", json: {} },
    ]);
  });

  it("defaults a cancel with no reason to the user's own marker", async () => {
    const calls: unknown[] = [];
    const rows = {
      sendJson: async (_method: string, _path: string, init: { json?: unknown }) => {
        calls.push(init.json);
        return { data: {} };
      },
      parse: () => ({ ok: true }),
    } as unknown as ApiClient;
    const transport = createCloudChatTransport({ getClient: () => rows });
    await transport.cancelChat("s-21");
    expect(calls).toEqual([{ reason: "user_cancelled" }]);
  });
});
