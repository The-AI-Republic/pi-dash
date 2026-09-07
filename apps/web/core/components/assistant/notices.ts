/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { IAssistantMessage, IAssistantSkippedServer } from "@pi-dash/types";

/**
 * Client-only `tool_servers_skipped` notices, and how they sit in a transcript.
 *
 * These are synthesized in the browser from an SSE event and never persisted,
 * which makes their ordering the whole problem: events and messages are
 * numbered by two independent per-thread counters (`_next_event_seq` /
 * `_next_message_seq` in `assistant/runtime/events.py`), and every streamed
 * delta consumes an event seq — so the event counter runs far ahead of the
 * message counter and the two spaces are *not* comparable.
 */

export type ById = Record<string, IAssistantMessage>;

export const isNotice = (m: IAssistantMessage): boolean => m.role === "notice";

// The event seq a notice was synthesized from. Real messages have unique
// integer seqs, so this only ever breaks ties between two notices anchored to
// the same message.
export const noticeEventSeq = (m: IAssistantMessage): number => {
  const seq = (m.payload as { event_seq?: unknown } | undefined)?.event_seq;
  return typeof seq === "number" ? seq : 0;
};

export const bySeq = (a: IAssistantMessage, b: IAssistantMessage): number =>
  a.seq - b.seq || noticeEventSeq(a) - noticeEventSeq(b);

/**
 * Add the notice for one `tool_servers_skipped` event.
 *
 * Anchored just after the highest *message* seq present when it arrives — using
 * the event seq directly would sort every notice below the newest turn — and
 * never moved again, so an SSE replay from 0 is idempotent.
 */
export function withNotice(prev: ById, eventSeq: number, servers: IAssistantSkippedServer[], createdAt: string): ById {
  const id = `notice:${eventSeq}`;
  if (prev[id]) return prev; // already anchored — replay must not re-anchor it
  let anchor = 0;
  for (const m of Object.values(prev)) if (!isNotice(m) && m.seq > anchor) anchor = m.seq;
  return {
    ...prev,
    [id]: {
      id,
      role: "notice",
      content: "",
      status: "completed",
      // Between the anchor message and the next one; ties broken by event_seq.
      seq: anchor + 0.5,
      turn_id: null,
      payload: { servers, event_seq: eventSeq },
      created_at: createdAt,
      completed_at: createdAt,
    },
  };
}

/**
 * The highest-seq real message, used to detect turn completion from polled
 * state when an SSE `turn_completed` is missed.
 *
 * Notices are excluded deliberately: they anchor *after* the message they
 * follow, so a notice would otherwise always be the max-seq item, never carry a
 * terminal role, and leave the composer disabled for the rest of the turn.
 */
export function latestRealMessage(list: IAssistantMessage[]): IAssistantMessage | undefined {
  let last: IAssistantMessage | undefined;
  for (const m of list) if (!isNotice(m) && (!last || m.seq > last.seq)) last = m;
  return last;
}
