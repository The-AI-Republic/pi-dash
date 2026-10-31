// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Skipped tool-server notices (parity/specs/agents.md §11, AGT-046). A run
// that drops tool servers emits one `tool_servers_skipped` event per turn;
// the view synthesizes an inline notice from it, one line per server, and
// never a toast. Notices are client-only — a reload re-derives them from
// the replayed event — and they anchor just after the highest message
// sequence present when they arrive, so a replay from zero re-anchors
// nothing and the notice keeps its place in the turn it belongs to.

import type { AssistantSkippedServer } from "@pidash/api-client";
import type { AssistantMessage } from "@pidash/api-client";

/** One client-synthesized notice, placed among the transcript rows. */
export interface ThreadNotice {
  /** Replay key: one notice per skipped-servers event. */
  id: string;
  /** Sort position, fractional so it sits between two message rows. */
  seq: number;
  /** The event sequence this notice was synthesized from. */
  eventSeq: number;
  servers: readonly AssistantSkippedServer[];
  createdAt: string;
}

/** The replay key one skipped-servers event anchors under. */
export function noticeId(eventSeq: number): string {
  return `notice:${eventSeq}`;
}

/**
 * The message sequence a notice arriving now anchors after: the highest
 * row sequence on screen, or zero when the transcript is still empty.
 */
export function noticeAnchorSeq(rows: Readonly<Record<string, AssistantMessage>>): number {
  let anchor = 0;
  for (const row of Object.values(rows)) {
    if (row.seq > anchor) anchor = row.seq;
  }
  return anchor;
}

/**
 * Machine-readable drop reasons the backend emits, in plain language. The
 * set covers the resolver's branches, the surfaced credential failures and
 * the cloud OpenHub reason; a runtime failure reports a raw exception class
 * name instead, so anything unmapped renders as it arrived.
 */
const REASON_TEXT: Readonly<Record<string, string>> = {
  url_blocked: "its URL is not allowed",
  auth_header_unreadable: "its credential could not be read",
  toolset_unavailable: "it could not be reached",
  toolsets_unavailable: "they could not be loaded",
  assistant_not_configured: "its credential could not be decrypted",
  openhub_unavailable: "OpenHub apps were unavailable",
};

function reasonText(reason: string): string {
  return REASON_TEXT[reason] ?? reason;
}

/**
 * One notice line for one dropped server. The resolver's total-failure
 * branch carries a placeholder name rather than a real server, so that
 * reason phrases as a whole-capability outage; the reason code names the
 * branch, never the user-supplied name.
 */
export function skippedServerLine(server: AssistantSkippedServer): string {
  const detail = reasonText(server.reason);
  if (server.reason === "toolsets_unavailable") {
    return `Tool servers were unavailable for this reply (${detail}).`;
  }
  return `Tool server ${server.name} was unavailable for this reply (${detail}).`;
}
