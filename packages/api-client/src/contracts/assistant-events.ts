// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant turn events: the parsed SSE union behind the live thread
// view, plus the frame splitter and the stream address. The socket
// itself stays with the platform stream — this contracts the bytes.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { AssistantMessage } from "./assistant-messages.js";
import { dateTime, nullableUuid, uuid } from "./fields.js";

const eventBase = {
  id: z.number(),
  thread: uuid(),
  message: nullableUuid(),
  seq: z.number(),
  created_at: dateTime(),
};

/** Token usage the completed turn reports; keys stay stable. */
export const AssistantTurnUsage = z.object({
  input_tokens: z.optional(z.nullable(z.number())),
  output_tokens: z.optional(z.nullable(z.number())),
  total_tokens: z.optional(z.nullable(z.number())),
  requests: z.optional(z.nullable(z.number())),
  tool_calls: z.optional(z.nullable(z.number())),
});
export type AssistantTurnUsage = z.infer<typeof AssistantTurnUsage>;

/** One tool server a run dropped, with the machine-readable reason. */
export const AssistantSkippedServer = z.object({
  name: z.string(),
  reason: z.string(),
});
export type AssistantSkippedServer = z.infer<typeof AssistantSkippedServer>;

const TurnStartedEvent = z.object({
  ...eventBase,
  kind: z.literal("turn_started"),
  payload: z.object({ turn_id: uuid() }),
});

const AssistantDeltaEvent = z.object({
  ...eventBase,
  kind: z.literal("assistant_delta"),
  payload: z.object({
    turn_id: uuid(),
    params: z.object({ delta: z.string() }),
  }),
});

const MessageEnvelopeEvent = <K extends "message_created" | "message_completed" | "tool_call" | "tool_result">(
  kind: K
) =>
  z.object({
    ...eventBase,
    kind: z.literal(kind),
    payload: z.object({
      turn_id: uuid(),
      message: AssistantMessage,
    }),
  });

const MessageCreatedEvent = MessageEnvelopeEvent("message_created");
const MessageCompletedEvent = MessageEnvelopeEvent("message_completed");
// Never emitted by this backend, but the transcript renders the role
// and the stream contract reserves the kind — parse it, don't drop it.
const ToolCallEvent = MessageEnvelopeEvent("tool_call");
const ToolResultEvent = MessageEnvelopeEvent("tool_result");

const ToolServersSkippedEvent = z.object({
  ...eventBase,
  kind: z.literal("tool_servers_skipped"),
  payload: z.object({ servers: z.array(AssistantSkippedServer) }),
});

const TurnFailedEvent = z.object({
  ...eventBase,
  kind: z.literal("turn_failed"),
  payload: z.object({
    turn_id: uuid(),
    error_code: z.string(),
    detail: z.string(),
  }),
});

const TurnCompletedEvent = z.object({
  ...eventBase,
  kind: z.literal("turn_completed"),
  payload: z.object({
    turn_id: uuid(),
    usage: AssistantTurnUsage,
  }),
});

const TurnCancelledEvent = z.object({
  ...eventBase,
  kind: z.literal("turn_cancelled"),
  payload: z.object({ turn_id: uuid() }),
});

// Every event kind the backend emits, narrowed by kind. A plain union,
// not a discriminated one: both wrap the same schema instances, so parsing
// and the narrowed `kind` results are identical, but a discriminated union
// would add its runtime to every bundle that mentions this contract — the
// web shell only re-exports the module, and the size gate caught it there.
export const KnownAssistantEvent = z.union([
  TurnStartedEvent,
  AssistantDeltaEvent,
  MessageCreatedEvent,
  MessageCompletedEvent,
  ToolCallEvent,
  ToolResultEvent,
  ToolServersSkippedEvent,
  TurnFailedEvent,
  TurnCompletedEvent,
  TurnCancelledEvent,
]);
export type KnownAssistantEvent = z.infer<typeof KnownAssistantEvent>;

/** A future kind: envelope known, payload opaque. Never dropped. */
export const UnknownAssistantEvent = z.object({
  ...eventBase,
  kind: z.string(),
  payload: z.unknown(),
});
export type UnknownAssistantEvent = z.infer<typeof UnknownAssistantEvent>;

/**
 * The parsed event: known kinds narrow, unknown kinds fall through
 * to the opaque envelope so a new server kind never breaks the view.
 */
export const AssistantEvent = z.union([KnownAssistantEvent, UnknownAssistantEvent]);
export type AssistantEvent = z.infer<typeof AssistantEvent>;

/**
 * Split SSE bytes into one parsed JSON frame per data block. Comment
 * lines (keepalives) and the event-name line are skipped; a data
 * block that is not JSON throws naming the frame — a corrupt frame
 * must surface, never silently drop the turn it belongs to.
 */
export function parseAssistantEventFrames(text: string): unknown[] {
  const frames: unknown[] = [];
  for (const block of text.split(/\r?\n\r?\n/)) {
    const data: string[] = [];
    for (const line of block.split(/\r?\n/)) {
      if (line.startsWith(":")) continue;
      if (line.startsWith("data:")) data.push(line.slice("data:".length).replace(/^ /, ""));
    }
    if (data.length === 0) continue;
    const raw = data.join("\n");
    try {
      frames.push(JSON.parse(raw) as unknown);
    } catch {
      throw new Error(`assistant event frame is not JSON: ${raw.slice(0, 120)}`);
    }
  }
  return frames;
}

/**
 * The stream address; `after` replays persisted events past that
 * sequence number first, so reconnects dedupe without losing rows.
 */
export function assistantEventsUrl(client: ApiClient, slug: string, threadId: string, after = 0): string {
  const url = `${client.baseUrl}/api/workspaces/${slug}/ai-assistant/threads/${threadId}/events/`;
  return after > 0 ? `${url}?after=${after}` : url;
}
