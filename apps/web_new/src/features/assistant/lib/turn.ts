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
}

export const EMPTY_TRANSCRIPT: TranscriptState = { rows: {} };

/**
 * Whether `incoming` may replace `current`: a terminal row always wins,
 * otherwise the longer text wins. A reconnect replays `message_created`
 * with the row as it looked at creation (empty, streaming), so replacing
 * unconditionally would wipe everything the deltas have built since.
 */
function wins(incoming: AssistantMessage, current: AssistantMessage | undefined): boolean {
  if (current === undefined) return true;
  return incoming.status !== "streaming" || incoming.content.length >= current.content.length;
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
    if (wins(incoming, rows[incoming.id])) {
      rows[incoming.id] = incoming;
      changed = true;
    }
  }
  return changed ? { rows } : state;
}

/** Merge a single row (the send's echoed user message, an event payload). */
export function mergeMessage(state: TranscriptState, message: AssistantMessage): TranscriptState {
  if (!wins(message, state.rows[message.id])) return state;
  return { rows: { ...state.rows, [message.id]: message } };
}

/**
 * Apply one parsed stream event. `seen` collects applied delta sequences so
 * a reconnect (which replays from the beginning) appends each chunk once.
 * Unknown kinds and events with no row are ignored; the caller never needs
 * to special-case a future server kind.
 */
export function applyAssistantEvent(state: TranscriptState, event: AssistantEvent, seen: Set<number>): TranscriptState {
  switch (event.kind) {
    case "message_created":
    case "message_completed":
    case "tool_call":
    case "tool_result": {
      const message = (event.payload as { message?: AssistantMessage }).message;
      return message === undefined ? state : mergeMessage(state, message);
    }
    case "assistant_delta": {
      if (seen.has(event.seq)) return state;
      const target = event.message;
      const delta = (event.payload as { params?: { delta?: unknown } }).params?.delta;
      if (target === null || typeof delta !== "string" || delta === "") return state;
      const current = state.rows[target];
      if (current === undefined) return state;
      seen.add(event.seq);
      return {
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
