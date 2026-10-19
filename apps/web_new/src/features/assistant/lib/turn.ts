// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Thread transcript rules (parity/specs/agents.md §10, AGT-042/AGT-047).
// Two feeds describe one conversation: the stored transcript page and the
// live event stream. Both speak the same message rows and both carry a
// sequence number, so the view keeps a row table keyed by id, merges the
// transcript without discarding streamed text that is ahead of it, and
// dedupes replayed event frames by sequence. Pure functions only; the
// hook owns timing.

import type { AssistantEvent, AssistantMessage } from "@pidash/api-client";

/** Chat roles the transcript draws; tool rows are carried for AGT-046. */
const REAL_ROLES = new Set(["user", "assistant", "error", "tool_call", "tool_result"]);

/** Rows by id; the render order always comes from the sequence number. */
export interface TranscriptState {
  readonly rows: Readonly<Record<string, AssistantMessage>>;
  /**
   * Highest delta sequence already appended. The channel replays from zero
   * on every reconnect, so a chunk at or below this was served before. It
   * lives in the state (not a ref) because the reducer must stay pure:
   * React replays a reducer to detect impurity, and a set mutated in place
   * made the replay skip the first delta and commit nothing.
   */
  readonly appliedDelta: number;
}

export const EMPTY_TRANSCRIPT: TranscriptState = { rows: {}, appliedDelta: 0 };

/**
 * Whether `incoming` may replace `current`. A terminal row always wins;
 * between two streaming copies the longer text wins, because a reconnect
 * replays `message_created` with the row as it looked at creation (empty).
 */
function wins(incoming: AssistantMessage, current: AssistantMessage | undefined): boolean {
  if (current === undefined) return true;
  if (incoming.status === "streaming") return incoming.content.length >= current.content.length;
  return true;
}

/**
 * Keep the text already on screen when the incoming row has none. A failed
 * or cancelled finalize can rebuild the row with its stored content empty
 * (the soft-time-limit and sweep paths do not rewrite it) while the streamed
 * partial answer is what the reader saw; the status still moves forward.
 */
function carryText(incoming: AssistantMessage, current: AssistantMessage | undefined): AssistantMessage {
  if (current === undefined || incoming.content !== "" || current.content === "") return incoming;
  return { ...incoming, content: current.content };
}

/**
 * Merge one stored page into the table. A live row that is already ahead of
 * the page (more text, or already terminal) is kept: polling must never
 * rewind a message the stream has been building.
 */
export function mergeTranscript(state: TranscriptState, page: readonly AssistantMessage[]): TranscriptState {
  let changed = false;
  const rows: Record<string, AssistantMessage> = { ...state.rows };
  for (const incoming of page) {
    const current = rows[incoming.id];
    if (wins(incoming, current)) {
      rows[incoming.id] = carryText(incoming, current);
      changed = true;
    }
  }
  return changed ? { ...state, rows } : state;
}

/** Merge a single row (the send's echoed user message, an event payload). */
export function mergeMessage(state: TranscriptState, message: AssistantMessage): TranscriptState {
  const current = state.rows[message.id];
  if (!wins(message, current)) return state;
  return { ...state, rows: { ...state.rows, [message.id]: carryText(message, current) } };
}

/**
 * Apply one parsed stream event. Two rules keep a reconnect (which replays
 * the thread from the beginning) from doubling text: a delta at or below
 * the last applied sequence was already served, and a delta never appends
 * to a row that has already stopped streaming, because its stored content
 * is the whole answer. Unknown kinds and events with no row are ignored;
 * the caller never needs to special-case a future server kind.
 */
export function applyAssistantEvent(state: TranscriptState, event: AssistantEvent): TranscriptState {
  switch (event.kind) {
    case "message_created":
    case "message_completed":
    case "tool_call":
    case "tool_result": {
      const message = (event.payload as { message?: AssistantMessage }).message;
      return message === undefined ? state : mergeMessage(state, message);
    }
    case "assistant_delta": {
      if (event.seq <= state.appliedDelta) return state;
      const target = event.message;
      const delta = (event.payload as { params?: { delta?: unknown } }).params?.delta;
      if (target === null || typeof delta !== "string" || delta === "") return state;
      const current = state.rows[target];
      if (current === undefined || current.status !== "streaming") return state;
      return {
        appliedDelta: event.seq,
        rows: { ...state.rows, [target]: { ...current, content: current.content + delta } },
      };
    }
    default:
      return state;
  }
}

/** Rows oldest first; the sequence number is the server's ordering. */
export function transcriptRows(state: TranscriptState): AssistantMessage[] {
  return Object.values(state.rows)
    .filter((row) => REAL_ROLES.has(row.role))
    .sort((left, right) => left.seq - right.seq);
}

/**
 * Whether a running turn has visibly finished: the newest real row is an
 * assistant reply or an error and nothing is still streaming. This is the
 * poll fallback the transcript uses when the event stream is silent; the
 * newest row is what matters, so an older streamed row never traps it.
 */
export function replySettled(rows: readonly AssistantMessage[]): boolean {
  const last = rows.at(-1);
  if (last === undefined) return false;
  if (last.role !== "assistant" && last.role !== "error") return false;
  return last.status !== "streaming";
}

/** Text a bubble shows when its content is still empty. */
export function streamingPlaceholder(status: AssistantMessage["status"]): string {
  return status === "streaming" ? "…" : status;
}
