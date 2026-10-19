// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Event channel (AGT-042): payloads parse onto the union, unknown kinds
// fall through, heartbeats and other event names are ignored, and a corrupt
// frame is reported without killing the subscription.
import type { ApiClient } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

import type { EventStream, StreamMessage } from "../../../core/platform/index.js";
import { readAssistantEvents, subscribeAssistantEvents } from "./events.js";

const client = { baseUrl: "https://api.test" } as ApiClient;

const THREAD = "11111111-1111-4111-8111-111111111111";
const TURN = "44444444-4444-4444-8444-444444444444";

/** The `data:` payload the platform reader hands over. */
function payload(seq: number, kind: string, body: Record<string, unknown>): string {
  return JSON.stringify({
    id: seq,
    thread: THREAD,
    message: null,
    seq,
    kind,
    payload: body,
    created_at: "2026-01-01T00:00:00Z",
  });
}

function feedOf(messages: StreamMessage[]): EventStream & { closed: boolean } {
  let closed = false;
  return {
    get closed() {
      return closed;
    },
    async *messages() {
      for (const message of messages) yield message;
    },
    close() {
      closed = true;
    },
    error: undefined,
  };
}

describe("readAssistantEvents", () => {
  it("parses a payload and keeps unknown kinds on the opaque envelope", () => {
    const started = readAssistantEvents(payload(1, "turn_started", { turn_id: TURN }));
    const future = readAssistantEvents(payload(2, "future_kind", { x: 1 }));
    expect([...started, ...future].map((event) => event.kind)).toEqual(["turn_started", "future_kind"]);
  });

  it("parses one payload and raw SSE blocks alike", () => {
    const one = readAssistantEvents(payload(1, "turn_started", { turn_id: TURN }));
    const wire = readAssistantEvents(`event: chat.event\ndata: ${payload(2, "turn_cancelled", { turn_id: TURN })}\n\n`);
    expect(one.map((event) => event.seq)).toEqual([1]);
    expect(wire.map((event) => event.seq)).toEqual([2]);
  });

  it("returns nothing for a heartbeat-only payload", () => {
    expect(readAssistantEvents(": keepalive\n\n")).toEqual([]);
  });

  it("throws on a payload that is not JSON", () => {
    expect(() => readAssistantEvents("{not json")).toThrow(/not JSON/);
  });

  it("throws when the payload does not match the contract", () => {
    expect(() => readAssistantEvents('{"kind":"turn_started"}')).toThrow(/did not match/);
  });
});

describe("subscribeAssistantEvents", () => {
  it("delivers chat.event frames and ignores other event names", async () => {
    const onEvent = vi.fn();
    const feed = feedOf([
      { event: "ping", data: payload(1, "turn_started", { turn_id: TURN }) },
      { event: "chat.event", data: payload(2, "turn_completed", { turn_id: TURN, usage: {} }) },
    ]);
    const openStream = vi.fn((_url: string, _init: unknown) => feed);
    subscribeAssistantEvents("acme", "thread-1", 0, { onEvent }, () => client, openStream);
    await vi.waitFor(() => expect(onEvent).toHaveBeenCalledTimes(1));
    expect(onEvent.mock.calls[0]?.[0]).toMatchObject({ kind: "turn_completed", seq: 2 });
    const [url, init] = openStream.mock.calls[0] ?? [];
    expect(url).toBe("https://api.test/api/workspaces/acme/ai-assistant/threads/thread-1/events/");
    expect(init).toEqual({ method: "GET", headers: { Accept: "text/event-stream" } });
  });

  it("appends the replay cursor when resuming", () => {
    const openStream = vi.fn((_url: string, _init: unknown) => feedOf([]));
    subscribeAssistantEvents("acme", "t1", 41, { onEvent: vi.fn() }, () => client, openStream);
    expect(openStream.mock.calls[0]?.[0]).toBe(
      "https://api.test/api/workspaces/acme/ai-assistant/threads/t1/events/?after=41"
    );
  });

  it("reports a corrupt frame and keeps reading", async () => {
    const onEvent = vi.fn();
    const onError = vi.fn();
    const feed = feedOf([
      { event: "chat.event", data: "{broken" },
      { event: "chat.event", data: payload(3, "turn_cancelled", { turn_id: TURN }) },
    ]);
    subscribeAssistantEvents(
      "acme",
      "t1",
      0,
      { onEvent, onError },
      () => client,
      () => feed
    );
    await vi.waitFor(() => expect(onEvent).toHaveBeenCalledTimes(1));
    expect(onError).toHaveBeenCalledTimes(1);
  });

  it("reports a broken channel", async () => {
    const onError = vi.fn();
    const feed: EventStream = {
      messages: async function* () {
        yield { event: "ping", data: "" };
        throw new Error("socket closed");
      },
      close: vi.fn(),
      error: undefined,
    };
    subscribeAssistantEvents(
      "acme",
      "t1",
      0,
      { onEvent: vi.fn(), onError },
      () => client,
      () => feed
    );
    await vi.waitFor(() => expect(onError).toHaveBeenCalledTimes(1));
  });

  it("reopens the channel when it ends on its own", async () => {
    const onEvent = vi.fn();
    const feeds = [feedOf([]), feedOf([{ event: "chat.event", data: payload(4, "turn_started", { turn_id: TURN }) }])];
    const openStream = vi.fn(() => feeds.shift() ?? feedOf([]));
    const subscription = subscribeAssistantEvents("acme", "t1", 0, { onEvent }, () => client, openStream);
    await vi.waitFor(() => expect(onEvent).toHaveBeenCalledTimes(1), { timeout: 3000 });
    expect(openStream).toHaveBeenCalledTimes(2);
    subscription.close();
  });

  it("drops a frame that lands after the caller closed the channel", async () => {
    const onEvent = vi.fn();
    let release: (() => void) | undefined;
    const feed: EventStream = {
      messages: async function* () {
        // Hold the first frame until the caller has closed the channel.
        await new Promise<void>((resolve) => {
          release = resolve;
        });
        yield { event: "chat.event", data: payload(1, "turn_started", { turn_id: TURN }) };
      },
      close: vi.fn(() => release?.()),
      error: undefined,
    };
    const subscription = subscribeAssistantEvents(
      "acme",
      "t1",
      0,
      { onEvent },
      () => client,
      () => feed
    );
    subscription.close();
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(onEvent).not.toHaveBeenCalled();
  });

  it("closes once and does not report a close it caused itself", async () => {
    const onError = vi.fn();
    let release: (() => void) | undefined;
    const feed: EventStream = {
      messages: async function* () {
        await new Promise<void>((resolve) => {
          release = resolve;
        });
        yield { event: "chat.event", data: payload(5, "turn_cancelled", { turn_id: TURN }) };
        throw new Error("aborted");
      },
      close: vi.fn(() => release?.()),
      error: undefined,
    };
    const subscription = subscribeAssistantEvents(
      "acme",
      "t1",
      0,
      { onEvent: vi.fn(), onError },
      () => client,
      () => feed
    );
    subscription.close();
    subscription.close();
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(feed.close).toHaveBeenCalledTimes(1);
    expect(onError).not.toHaveBeenCalled();
  });
});
