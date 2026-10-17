// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Event union, frame splitter and stream address against stubbed
// frames (no backend, no socket).
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import {
  AssistantEvent,
  KnownAssistantEvent,
  assistantEventsUrl,
  parseAssistantEventFrames,
} from "./assistant-events.js";

const THREAD = "123e4567-e89b-12d3-a456-426614174000";
const MESSAGE = "223e4567-e89b-12d3-a456-426614174001";
const TURN = "323e4567-e89b-12d3-a456-426614174002";
const AT = "2026-10-16T15:32:05.479661+09:00";

const ENVELOPE = {
  id: 7,
  thread: THREAD,
  message: MESSAGE,
  seq: 9,
  created_at: AT,
};

const MESSAGE_ROW = {
  id: MESSAGE,
  role: "assistant",
  content: "hello",
  status: "completed",
  seq: 3,
  turn_id: TURN,
  payload: {},
  created_at: AT,
  completed_at: AT,
};

const KNOWN: { kind: string; payload: unknown }[] = [
  { kind: "turn_started", payload: { turn_id: TURN } },
  { kind: "assistant_delta", payload: { turn_id: TURN, params: { delta: "hel" } } },
  { kind: "message_created", payload: { turn_id: TURN, message: MESSAGE_ROW } },
  { kind: "message_completed", payload: { turn_id: TURN, message: MESSAGE_ROW } },
  { kind: "tool_call", payload: { turn_id: TURN, message: MESSAGE_ROW } },
  { kind: "tool_result", payload: { turn_id: TURN, message: MESSAGE_ROW } },
  {
    kind: "tool_servers_skipped",
    payload: { servers: [{ name: "jira", reason: "connect_failed" }] },
  },
  { kind: "turn_failed", payload: { turn_id: TURN, error_code: "provider_unreachable", detail: "down" } },
  {
    kind: "turn_completed",
    payload: { turn_id: TURN, usage: { input_tokens: 10, output_tokens: 4, total_tokens: 14 } },
  },
  { kind: "turn_cancelled", payload: { turn_id: TURN } },
];

describe("AssistantEvent", () => {
  it.each(KNOWN)("accepts the $kind frame", ({ kind, payload }) => {
    expect(AssistantEvent.parse({ ...ENVELOPE, kind, payload }).kind).toBe(kind);
  });

  it("narrows known payloads by kind", () => {
    const delta = KnownAssistantEvent.parse({
      ...ENVELOPE,
      kind: "assistant_delta",
      payload: { turn_id: TURN, params: { delta: "hel" } },
    });
    expect(delta.kind).toBe("assistant_delta");
    if (delta.kind === "assistant_delta") expect(delta.payload.params.delta).toBe("hel");

    const skipped = KnownAssistantEvent.parse({
      ...ENVELOPE,
      kind: "tool_servers_skipped",
      payload: { servers: [{ name: "jira", reason: "connect_failed" }] },
    });
    if (skipped.kind === "tool_servers_skipped") expect(skipped.payload.servers).toHaveLength(1);

    const failed = KnownAssistantEvent.parse({
      ...ENVELOPE,
      kind: "turn_failed",
      payload: { turn_id: TURN, error_code: "provider_unreachable", detail: "down" },
    });
    if (failed.kind === "turn_failed") expect(failed.payload.error_code).toBe("provider_unreachable");
  });

  it("falls an unknown kind through to the opaque envelope", () => {
    const event = AssistantEvent.parse({ ...ENVELOPE, kind: "summaries_ready", payload: { n: 2 } });
    expect(event.kind).toBe("summaries_ready");
    if (event.kind === "summaries_ready") expect(event.payload).toEqual({ n: 2 });
  });

  it("rejects a frame without the envelope", () => {
    expect(() => AssistantEvent.parse({ kind: "turn_started", payload: {} })).toThrow();
  });
});

describe("parseAssistantEventFrames", () => {
  it("skips keepalives and parses one frame per data block", () => {
    const first = JSON.stringify({ ...ENVELOPE, kind: "turn_started", payload: { turn_id: TURN } });
    const second = JSON.stringify({
      ...ENVELOPE,
      seq: 10,
      kind: "turn_completed",
      payload: { turn_id: TURN, usage: {} },
    });
    const frames = parseAssistantEventFrames(
      `: keepalive\n\nevent: chat.event\ndata: ${first}\n\nevent: chat.event\ndata: ${second}\n\n`
    );
    expect(frames).toHaveLength(2);
    expect(AssistantEvent.parse(frames[0]).kind).toBe("turn_started");
    expect(AssistantEvent.parse(frames[1]).kind).toBe("turn_completed");
  });

  it("joins multi-line data and tolerates CRLF", () => {
    const frames = parseAssistantEventFrames('event: chat.event\r\ndata: {"a":\r\ndata: 1}\r\n\r\n');
    expect(frames).toEqual([{ a: 1 }]);
  });

  it("strips at most one space after the data marker", () => {
    expect(parseAssistantEventFrames('data:{"b":2}\n\n')).toEqual([{ b: 2 }]);
    expect(parseAssistantEventFrames('data:  {"b": 3}\n\n')).toEqual([{ b: 3 }]);
  });

  it("returns nothing for empty or comment-only bytes", () => {
    expect(parseAssistantEventFrames("")).toEqual([]);
    expect(parseAssistantEventFrames(": keepalive\n\n: keepalive\n\n")).toEqual([]);
  });

  it("throws naming a corrupt data block", () => {
    expect(() => parseAssistantEventFrames("event: chat.event\ndata: {oops\n\n")).toThrow(
      /assistant event frame is not JSON/
    );
  });
});

describe("assistantEventsUrl", () => {
  const client = { baseUrl: "https://api.test" } as ApiClient;

  it("addresses the thread stream, replaying past the cursor when given", () => {
    expect(assistantEventsUrl(client, "acme", THREAD)).toBe(
      `https://api.test/api/workspaces/acme/ai-assistant/threads/${THREAD}/events/`
    );
    expect(assistantEventsUrl(client, "acme", THREAD, 12)).toBe(
      `https://api.test/api/workspaces/acme/ai-assistant/threads/${THREAD}/events/?after=12`
    );
  });
});
