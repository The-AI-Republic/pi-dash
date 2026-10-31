// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud god-mode entry decision (ADM-074-079, from
// parity/specs/admin.md §10). Pure, so the resolver and tests share it:
// the admin check plus a one-shot auth probe resolve to exactly one of
// spinner, OIDC bounce, terminal denied, or retryable error — never by
// instance setup state, and never by guessing on ambiguity (the
// loop-avoiding classification behind NEWFRONT-496 is load-bearing).
import type { AdminSession, CloudAuthProbe } from "@pidash/api-client";

/** Admin-check verdicts the entry branches on. */
export type CloudEntrySessionState = "pending" | "admin" | "anonymous" | "forbidden" | "error";

/** Probe verdicts the forbidden branch classifies with. */
export type CloudEntryProbeState = "pending" | "authenticated" | "anonymous" | "inconclusive";

/** The four cloud entry outcomes (ADM-074). */
export type CloudEntryOutcome = "spinner" | "bounce" | "denied" | "retry";

/** The settled-or-not shape the session mapper reads off the admin query. */
export interface CloudEntrySessionInput {
  readonly isPending: boolean;
  readonly isError: boolean;
  readonly data: AdminSession | undefined;
}

/** The settled-or-not shape the probe mapper reads off the probe query. */
export interface CloudEntryProbeInput {
  readonly isPending: boolean;
  readonly isError: boolean;
  readonly data: CloudAuthProbe | undefined;
}

/**
 * Map the admin check to its verdict. A failed check reads as an
 * outage, never as anonymous; an unsettled one stays pending.
 */
export function toCloudEntrySession(query: CloudEntrySessionInput): CloudEntrySessionState {
  if (query.isError) return "error";
  if (query.isPending || query.data === undefined) return "pending";
  return query.data.kind;
}

/**
 * Map the probe to its verdict. A failed probe read (contract drift,
 * caller abort) is inconclusive: the gate retries rather than bounce
 * or deny on ambiguity, so a flaky probe cannot start an
 * OIDC-bounce loop.
 */
export function toCloudEntryProbe(query: CloudEntryProbeInput): CloudEntryProbeState {
  if (query.isError) return "inconclusive";
  if (query.isPending || query.data === undefined) return "pending";
  return query.data.kind;
}

/**
 * Resolve the cloud entry to its single outcome. An authorized admin
 * holds the spinner while the home guard replace-navigates to general
 * settings (ADM-075); an anonymous read (401) already proves no
 * session, so it bounces without probing (ADM-076); a forbidden read
 * (403) lets the probe decide between bounce, deny, and retry
 * (ADM-076/077/079); a failed check retries (ADM-078).
 */
export function decideCloudEntry(session: CloudEntrySessionState, probe: CloudEntryProbeState): CloudEntryOutcome {
  if (session === "pending" || session === "admin") return "spinner";
  if (session === "error") return "retry";
  if (session === "anonymous") return "bounce";
  if (probe === "authenticated") return "denied";
  if (probe === "anonymous") return "bounce";
  if (probe === "pending") return "spinner";
  return "retry";
}

/**
 * The OIDC round-trip return: the current path plus query as a
 * relative reference, so a deep god-mode link survives the bounce and
 * lands back on the same URL (ADM-076).
 */
export function cloudEntryReturnTo(pathname: string, search: string): string {
  return `${pathname}${search}`;
}
