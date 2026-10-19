// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Event-feed rules for the runner chat (NEWFRONT-551, RUN-029). One
// streamed frame is read here — how a delta reaches the transcript, which
// frame ends or refreshes a turn, what a frame says in the activity strip,
// and how the strip is capped and collapsed. Keeping this pure lets every
// branch be pinned without a server or a transport.

import type { ChatEvent, ChatMessage } from "@pidash/api-client";

/** How long an event may predate the subscription and still count as live. */
export const REALTIME_GRACE_MS = 500;

/** Most activity rows the strip keeps. */
export const ACTIVITY_STRIP_LIMIT = 6;

/** Frames that end a turn or otherwise change what the page must re-read. */
const TERMINAL_KINDS = new Set(["turn_completed", "chat_failed", "chat_closed"]);
const REFRESH_KINDS = new Set(["turn_started", "turn_completed", "chat_failed", "chat_closed", "chat_warmed"]);

/** Frames the strip never shows: deltas render in the transcript, the rest is bookkeeping. */
const STRIP_EXCLUDED_KINDS = new Set([
  "assistant_delta",
  "turn_completed",
  "chat_closed",
  "chat_warmed",
  "chat_timing",
]);

/** Named frames with a fixed meaning; `null` suppresses lifecycle narration. */
const NAMED_LABELS: Record<string, string | null> = {
  chat_approval_request: "Waiting for your approval",
  chat_failed: "Failed",
  message_started: null,
  turn_started: null,
  run_started: null,
};

/** Engine methods that narrate the session rather than show work. */
const NARRATION_METHODS = new Set([
  "thread/started",
  "turn/started",
  "turn/completed",
  "remoteControl/status/changed",
  "thread/status/changed",
  "thread/tokenUsage/updated",
  "account/rateLimits/updated",
  "mcpServer/startupStatus/updated",
]);

/** Item types the transcript already renders, so the strip would only repeat them. */
const TRANSCRIPT_ITEMS = new Set(["userMessage", "agentMessage", "reasoning"]);

/** Read the delta text out of an assistant frame's payload. */
export function assistantDeltaText(payload: Record<string, unknown>): string {
  const params = payload.params;
  if (params === undefined || params === null || typeof params !== "object" || Array.isArray(params)) return "";
  const record = params as Record<string, unknown>;
  const delta = record.delta;
  if (typeof delta === "string") return delta;
  if (delta !== null && typeof delta === "object" && !Array.isArray(delta)) {
    const text = (delta as Record<string, unknown>).text;
    if (typeof text === "string") return text;
  }
  return typeof record.text === "string" ? record.text : "";
}

/**
 * True when a frame was produced after this subscription opened (with a
 * small grace for clock skew). A replay of older stored frames is still
 * shown in the strip, but must not be appended to the live transcript,
 * which the server read already carries.
 */
export function isRealtimeEvent(event: Pick<ChatEvent, "created_at">, subscribedAtMs: number): boolean {
  const at = Date.parse(event.created_at);
  return Number.isFinite(at) && at >= subscribedAtMs - REALTIME_GRACE_MS;
}

/** True for a frame that ends a turn. */
export function isTerminalEvent(kind: string): boolean {
  return TERMINAL_KINDS.has(kind);
}

/** True for a frame that means the saved history should be re-read. */
export function refreshesHistory(kind: string): boolean {
  return REFRESH_KINDS.has(kind);
}

/**
 * Apply one delta to the transcript. A row already on screen grows only
 * while it still streams; a delta for a row the read has not delivered
 * yet starts a new streaming bubble at the end. A finished row is left
 * alone — the history read is authoritative once a turn is over.
 */
export function applyAssistantDelta(
  messages: readonly ChatMessage[],
  event: Pick<ChatEvent, "message" | "session" | "created_at" | "payload">,
  sessionId: string
): ChatMessage[] {
  const messageId = event.message;
  if (messageId === null) return [...messages];
  const delta = assistantDeltaText(event.payload);
  if (delta === "") return [...messages];
  const existing = messages.findIndex((message) => message.id === messageId);
  if (existing !== -1) {
    const row = messages[existing]!;
    if (row.status !== "streaming") return [...messages];
    const next = [...messages];
    next[existing] = { ...row, content: `${row.content}${delta}` };
    return next;
  }
  const seq = messages.reduce((max, message) => Math.max(max, message.seq), 0) + 1;
  return [
    ...messages,
    {
      id: messageId,
      session: sessionId,
      role: "assistant",
      content: delta,
      content_parts: [],
      status: "streaming",
      local_item_id: "",
      local_turn_id: "",
      seq,
      created_at: event.created_at,
      completed_at: null,
    },
  ];
}

/**
 * What one frame is doing, for the activity strip, or null to show
 * nothing. Commands, edits, warnings, approvals and failures matter;
 * lifecycle chatter and repeated transcript rows do not. Engine methods
 * the daemon did not classify arrive as `kind: "raw"`, so the payload's
 * method is read rather than the kind.
 */
export function activityLabel(event: Pick<ChatEvent, "kind" | "payload">): string | null {
  if (event.kind !== "raw") {
    const named = NAMED_LABELS[event.kind];
    if (named !== undefined) return named;
    return STRIP_EXCLUDED_KINDS.has(event.kind) ? null : event.kind;
  }
  const record = event.payload as { method?: unknown; params?: unknown };
  const method = typeof record.method === "string" ? record.method : "";
  const params =
    record.params !== null && typeof record.params === "object" && !Array.isArray(record.params)
      ? (record.params as Record<string, unknown>)
      : {};
  const item =
    params.item !== null && typeof params.item === "object" && !Array.isArray(params.item)
      ? (params.item as Record<string, unknown>)
      : {};
  const itemType = typeof item.type === "string" ? item.type : "";
  switch (method) {
    case "warning":
      return typeof params.message === "string" && params.message !== "" ? `Warning: ${params.message}` : "Warning";
    case "item/started":
    case "item/completed": {
      const done = method === "item/completed";
      if (TRANSCRIPT_ITEMS.has(itemType)) return null;
      if (itemType === "commandExecution") {
        const command = typeof item.command === "string" && item.command !== "" ? item.command : "command";
        return `${done ? "Ran" : "Running"}: ${command}`;
      }
      if (itemType === "fileChange") {
        const path = typeof item.path === "string" && item.path !== "" ? item.path : "a file";
        return `${done ? "Edited" : "Editing"} ${path}`;
      }
      return itemType === "" ? null : `${done ? "Finished" : "Started"} ${itemType}`;
    }
    default:
      if (NARRATION_METHODS.has(method)) return null;
      return method === "" ? null : method;
  }
}

/** One activity row with the frame that produced it. */
export interface ActivityRow {
  seq: number;
  label: string;
}

/**
 * Identity of the action a frame reports, so a started frame and its
 * completed counterpart read as one entry. Non-raw frames are
 * identified by their label.
 */
function activityKey(event: Pick<ChatEvent, "kind" | "payload">, label: string): string {
  if (event.kind !== "raw") return label;
  const record = event.payload as { method?: unknown; params?: unknown };
  const params =
    record.params !== null && typeof record.params === "object" && !Array.isArray(record.params)
      ? (record.params as Record<string, unknown>)
      : {};
  const item =
    params.item !== null && typeof params.item === "object" && !Array.isArray(params.item)
      ? (params.item as Record<string, unknown>)
      : {};
  return `${String(item.type ?? "")}|${String(item.command ?? item.path ?? "")}`;
}

/**
 * The strip's rows. A started/completed pair for the same action reads
 * as one entry — the later wording wins — so a finished command shows
 * once; the newest few are kept.
 */
export function activityRows(events: readonly Pick<ChatEvent, "seq" | "kind" | "payload">[]): ActivityRow[] {
  const labelled: { row: ActivityRow; key: string }[] = [];
  for (const event of events) {
    const label = activityLabel(event);
    if (label !== null) labelled.push({ row: { seq: event.seq, label }, key: activityKey(event, label) });
  }
  const collapsed = labelled.filter(
    (entry, index) => index === labelled.length - 1 || labelled[index + 1]!.key !== entry.key
  );
  return collapsed.slice(-ACTIVITY_STRIP_LIMIT).map((entry) => entry.row);
}

/** Insert a frame once; a redelivered sequence is ignored. */
export function appendEvent<T extends Pick<ChatEvent, "seq">>(events: readonly T[], event: T): T[] {
  return events.some((item) => item.seq === event.seq) ? [...events] : [...events, event];
}

/** The highest sequence already seen, for resuming a dropped stream. */
export function lastSequence(events: readonly Pick<ChatEvent, "seq">[], applied: ReadonlySet<number>): number {
  let max = 0;
  for (const event of events) if (event.seq > max) max = event.seq;
  for (const seq of applied) if (seq > max) max = seq;
  return max;
}

/**
 * Deltas applied to the transcript: the sequence is recorded first so a
 * redelivery is dropped, and the delta lands only when it is both new and
 * live. A replay keeps its sequence marked, so it never double-applies
 * later either.
 */
export function applyDeltaOnce(input: {
  applied: ReadonlySet<number>;
  event: Pick<ChatEvent, "seq" | "message" | "session" | "created_at" | "payload">;
  subscribedAtMs: number;
  messages: readonly ChatMessage[];
  sessionId: string;
}): { applied: Set<number>; messages: ChatMessage[] } {
  const applied = new Set(input.applied);
  if (applied.has(input.event.seq)) return { applied, messages: [...input.messages] };
  applied.add(input.event.seq);
  if (!isRealtimeEvent(input.event, input.subscribedAtMs)) {
    return { applied, messages: [...input.messages] };
  }
  return { applied, messages: applyAssistantDelta(input.messages, input.event, input.sessionId) };
}
