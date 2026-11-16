// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collaboration channel frames (parity/specs/pages.md §8.6, §11.6,
// PAGE-069). Load and persist failures arrive as structured stateless
// payloads; oversized content additionally arrives as a force-close
// notice ahead of the dedicated close. Anything malformed, or addressed
// to another page, decodes to null and is ignored.

/** The two document lifecycle phases a failure interrupts. */
export type PageLiveFramePhase = "load" | "persist";

/** One decoded failure from the channel. */
export interface PageLiveErrorFrame {
  phase: PageLiveFramePhase;
  code: string;
  message: string;
  shouldDisconnect: boolean;
}

/** One decoded force-close notice: connections close right behind it. */
export interface PageLiveForceClose {
  reason: string;
  code: number;
  message: string;
}

export type PageLiveInbound =
  | { kind: "error"; error: PageLiveErrorFrame }
  | { kind: "force-close"; notice: PageLiveForceClose };

/** Close codes that end the session without another attempt. */
export const PAGE_LIVE_CLOSE_AUTH = [4401, 4403];

/** The server's force-close family (4000–4003). */
export const PAGE_LIVE_CLOSE_FORCE = [4000, 4001, 4002, 4003];

/** The dedicated oversized-content close and its reason. */
export const PAGE_LIVE_CLOSE_TOO_LARGE = 4001;
export const PAGE_LIVE_REASON_TOO_LARGE = "document_too_large";

/** How a socket close treats the session: terminal or worth a retry. */
export type PageLiveCloseKind = "auth" | "force" | "retryable";

/** Sort one close code into its session treatment. */
export function pageLiveCloseKind(code: number): PageLiveCloseKind {
  if (PAGE_LIVE_CLOSE_AUTH.includes(code)) return "auth";
  if (PAGE_LIVE_CLOSE_FORCE.includes(code)) return "force";
  return "retryable";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/**
 * Parse one stateless payload, tolerating the double-encoded envelope
 * the broadcast layer wraps frames in. Null when it is not JSON.
 */
function parsePayload(payload: string): unknown {
  let parsed: unknown;
  try {
    parsed = JSON.parse(payload) as unknown;
  } catch {
    return null;
  }
  if (typeof parsed === "string") {
    try {
      return JSON.parse(parsed) as unknown;
    } catch {
      return null;
    }
  }
  return parsed;
}

function errorFrameOf(event: Record<string, unknown>, pageId: string): PageLiveErrorFrame | null {
  const affected = event["affectedPages"];
  if (!isRecord(affected) || affected["currentPage"] !== pageId) return null;
  const data = event["data"];
  if (!isRecord(data)) return null;
  const message = data["error_message"];
  if (typeof message !== "string" || message === "") return null;
  const type = data["error_type"];
  if (type !== "fetch" && type !== "store") return null;
  const code = data["error_code"];
  return {
    phase: type === "fetch" ? "load" : "persist",
    code: typeof code === "string" && code !== "" ? code : "live_error",
    message,
    shouldDisconnect: data["should_disconnect"] === true,
  };
}

function forceCloseOf(event: Record<string, unknown>): PageLiveForceClose {
  const reason = event["reason"];
  const code = event["code"];
  const message = event["message"];
  return {
    reason: typeof reason === "string" ? reason : "",
    code: typeof code === "number" ? code : 4000,
    message: typeof message === "string" ? message : "",
  };
}

/**
 * Decode one inbound stateless payload for this page: an addressed
 * error event, a force-close notice, or null for anything else.
 */
export function parsePageLivePayload(payload: string, pageId: string): PageLiveInbound | null {
  const event = parsePayload(payload);
  if (!isRecord(event)) return null;
  if (event["type"] === "force_close") return { kind: "force-close", notice: forceCloseOf(event) };
  if (event["action"] !== "error") return null;
  const error = errorFrameOf(event, pageId);
  if (error === null) return null;
  return { kind: "error", error };
}
