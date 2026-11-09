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
//
// Turn-failure notices (NEWPIDASH-148, PRIVATEPI1-136) map the cloud
// backend's assistant turn error codes to their own inline text. The OSS
// edition leaves the free-quota gate off and stays verbatim.

import type { AssistantSkippedServer } from "@pidash/api-client";
import type { AssistantMessage } from "@pidash/api-client";

import { edition } from "../../../core/edition/index.js";
import { isOpenhubFreeQuotaEnabled } from "../../../core/edition/index.js";

const TURN_PAID_MODEL_DETAIL =
  "This model needs credit. The free model deepseek/deepseek-v4-flash is included on every plan.";
const TURN_WALLET_DETAIL = "Your OpenHub wallet is out of credits. Top up to keep using the assistant.";
const TURN_FREE_LANE_BUSY_DETAIL = "Too many free requests at once. Try again in a moment.";
const TURN_FREE_QUOTA_UNAVAILABLE_DETAIL = "Free quota is unavailable right now. Try again shortly.";

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

/** Turn-failure payload as emitted by `turn_failed` events. */
export interface TurnFailurePayload {
  error_code?: unknown;
  detail?: unknown;
  resets_at?: unknown;
  limit?: unknown;
  plan?: unknown;
}

/** Fallback when the event carries neither detail nor code. */
export const TURN_FAILURE_FALLBACK = "The assistant could not finish this reply.";

/**
 * Format a UTC `resets_at` (ISO 8601) for the user's locale and zone.
 * Returns the raw value when it does not parse, so the notice never
 * crashes on a malformed timestamp.
 */
export function formatResetsAt(resetsAt: string): string {
  const trimmed = resetsAt.trim();
  if (trimmed === "") return "";
  const parsed = new Date(trimmed);
  if (Number.isNaN(parsed.getTime())) return trimmed;
  try {
    return new Intl.DateTimeFormat(undefined, {
      dateStyle: "medium",
      timeStyle: "short",
      timeZone: undefined,
    }).format(parsed);
  } catch {
    return trimmed;
  }
}

/**
 * Extract the reset time from a `turn_failed` detail string when the
 * backend only sends a human-formatted detail (the Python classifier
 * formats `resets_at` server-side). Prefers a structured `resets_at`
 * field when the contract carries one; otherwise pulls the suffix
 * after "resets at" and re-formats it when it is parseable.
 */
export function freeQuotaResetText(payload: TurnFailurePayload, detail: string): string {
  if (typeof payload.resets_at === "string" && payload.resets_at.trim() !== "") {
    return formatResetsAt(payload.resets_at);
  }
  const prefix = "Daily free limit reached, resets at ";
  if (detail.startsWith(prefix)) {
    const suffix = detail
      .slice(prefix.length)
      .replace(/\.?\s*$/, "")
      .trim();
    if (suffix !== "" && suffix !== "00:00 UTC") {
      const formatted = formatResetsAt(suffix);
      if (formatted !== suffix) return formatted;
      // Already human-formatted server-side (e.g. "August 14, 2026 at
      // 07:00 PM CDT"); keep it verbatim — it is already in the user's
      // local zone.
      return suffix;
    }
    return suffix;
  }
  // Detail is the full sentence; pull the time after the last "resets at".
  const marker = "resets at ";
  const idx = detail.toLowerCase().lastIndexOf(marker);
  if (idx !== -1) {
    const suffix = detail
      .slice(idx + marker.length)
      .replace(/\.?\s*$/, "")
      .trim();
    if (suffix !== "") return suffix;
  }
  return detail;
}

/**
 * Map one `turn_failed` payload to the inline notice shown to the user.
 *
 * Cloud edition (`edition.id === "cloud"` or the free-quota flag) renders
 * each OpenHub code with its own wording per NEWPIDASH-148 / PRIVATEPI1-136:
 * free-quota-exhausted with the reset time localized, paid-model-needs-credit
 * pointing at the free model, wallet-empty top-up kept, and the retryable
 * busy/unavailable codes as "try again in a moment". OSS stays verbatim
 * (`detail` or `error_code`) so the community build never sees the mapping.
 *
 * Callers pass the raw payload; the function reads `error_code`/`detail`
 * and, for the exhausted code, `resets_at` (and the `detail` fallback).
 */
export function turnFailureNotice(payload: TurnFailurePayload): string {
  const detailRaw = typeof payload.detail === "string" ? payload.detail.trim() : "";
  const codeRaw = typeof payload.error_code === "string" ? payload.error_code.trim() : "";
  const detail = detailRaw !== "" ? (payload.detail as string) : "";
  const code = codeRaw !== "" ? (payload.error_code as string) : "";

  const verbatim = detailRaw !== "" ? detailRaw : codeRaw !== "" ? codeRaw : TURN_FAILURE_FALLBACK;

  if (!isOpenhubFreeQuotaEnabled(edition)) {
    return verbatim;
  }

  switch (code) {
    case "openhub_free_quota_exhausted": {
      const when = freeQuotaResetText(payload, detail);
      // The backend detail already reads "Daily free limit reached, resets
      // at {time}." — re-localize the time part so a UTC ISO from a
      // structured field renders in the viewer's zone.
      if (when !== "" && typeof payload.resets_at === "string" && payload.resets_at.trim() !== "") {
        return `Daily free limit reached, resets at ${when}.`;
      }
      // No structured field: keep the backend's already-localized detail.
      return detailRaw !== "" ? detailRaw : `Daily free limit reached, resets at ${when}.`;
    }
    case "openhub_paid_model_needs_credit":
      return TURN_PAID_MODEL_DETAIL;
    case "openhub_wallet_empty":
      return detailRaw !== "" ? detailRaw : TURN_WALLET_DETAIL;
    case "openhub_free_lane_busy":
      return detailRaw !== "" ? detailRaw : TURN_FREE_LANE_BUSY_DETAIL;
    case "openhub_free_quota_unavailable":
      return detailRaw !== "" ? detailRaw : TURN_FREE_QUOTA_UNAVAILABLE_DETAIL;
    default:
      return verbatim;
  }
}
