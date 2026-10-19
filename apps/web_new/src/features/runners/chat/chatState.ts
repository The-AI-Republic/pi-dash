// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner-chat page rules in one place (NEWFRONT-549, RUN-025/026/028/032):
// which conversation is on screen, how history entries read, when the page
// prepares a session, what the header says and why the composer is shut.
// Keeping these pure makes the page a thin composition and lets every
// branch be pinned by a unit test.

import type { ChatSession, Runner } from "@pidash/api-client";

import type { RunnerStatus } from "../status.js";
import { runnerDisplayStatus } from "../status.js";
import { timeAgo } from "../../../shared/lib/timeAgo.js";

/** Names the page shows while the runner record is still unknown. */
export const CHAT_RUNNER_FALLBACK_NAME = "Runner";

/** Composer gate reasons, in precedence order (RUN-032). */
export const COMPOSER_LOADING = "Loading";
export const COMPOSER_OFFLINE = "Runner offline";
export const COMPOSER_REVOKED = "Runner revoked";
export const COMPOSER_CLOSED = "Session closed";
export const COMPOSER_BUSY = "Response in progress";

export function isRunnerBusy(session: ChatSession | null | undefined): boolean {
  return Boolean(session?.active_message_id || session?.active_turn_id);
}

/**
 * Why the composer cannot take input, or null when it can (RUN-032). A
 * busy *runner* is deliberately absent: it serves chat alongside issue
 * work, so only an in-flight turn in this session blocks.
 */
export function composerBlockReason(
  runner: Pick<Runner, "status" | "enrolled_at"> | null | undefined,
  session: ChatSession | null | undefined
): string | null {
  if (!runner) return COMPOSER_LOADING;
  const status = runnerDisplayStatus(runner);
  if (status === "offline") return COMPOSER_OFFLINE;
  if (status === "revoked") return COMPOSER_REVOKED;
  if (session?.status === "closed") return COMPOSER_CLOSED;
  if (isRunnerBusy(session)) return COMPOSER_BUSY;
  return null;
}

/** Whether a runner can serve chat at all (RUN-028): online or busy. */
export function servesChat(runner: Pick<Runner, "status" | "enrolled_at">): boolean {
  const status = runnerDisplayStatus(runner);
  return status === "online" || status === "busy";
}

/** Activity stamp a history entry sorts and labels by. */
export function sessionActivityMs(session: Pick<ChatSession, "last_message_at" | "created_at">): number {
  const stamp = Date.parse(session.last_message_at ?? session.created_at);
  return Number.isFinite(stamp) ? stamp : 0;
}

/**
 * History entries, newest activity first, without touching the cached
 * array (RUN-026). A session with no activity yet falls back to its
 * creation time for ordering but reads as untouched.
 */
export function sortSessionsByActivity(sessions: readonly ChatSession[]): ChatSession[] {
  return [...sessions].sort((left, right) => sessionActivityMs(right) - sessionActivityMs(left));
}

const STAMP_FORMAT = new Intl.DateTimeFormat("en-US", {
  month: "short",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
});

/** Primary history label: when the conversation last moved. */
export function sessionTitle(session: Pick<ChatSession, "last_message_at" | "created_at">): string {
  const stamp = session.last_message_at ?? session.created_at;
  const parsed = Date.parse(stamp);
  if (!Number.isFinite(parsed)) return "Chat";
  return STAMP_FORMAT.format(parsed);
}

/** Secondary label: relative age, then a non-open status; untouched reads plainly. */
export function sessionSubtitle(
  session: Pick<ChatSession, "last_message_at" | "created_at" | "status">,
  nowMs?: number
): string {
  const suffix = session.status === "open" ? "" : ` · ${session.status}`;
  if (session.last_message_at === null) return `No turns yet${suffix}`;
  return `${timeAgo(session.last_message_at, nowMs)}${suffix}`;
}

export interface ChatHistoryEntry {
  id: string;
  title: string;
  subtitle: string;
  active: boolean;
}

export function buildHistoryEntries(
  sessions: readonly ChatSession[],
  activeId: string | undefined,
  nowMs?: number
): ChatHistoryEntry[] {
  return sortSessionsByActivity(sessions).map((session) => ({
    id: session.id,
    title: sessionTitle(session),
    subtitle: sessionSubtitle(session, nowMs),
    active: session.id === activeId,
  }));
}

export interface SelectionInput {
  sessions: readonly ChatSession[] | undefined;
  /** An explicit click or New chat, ahead of the address. */
  pendingId: string | null;
  /** `?sessionId=`. */
  requestedId: string | null;
  /** A session this page just created, not yet in the list. */
  precreated: ChatSession | null;
}

/**
 * The conversation to show (RUN-026). An explicit choice wins whenever it
 * resolves — even for a closed session, so history stays readable. A
 * stale or foreign id falls through to the automatic pick: the most
 * recent open session that has turns, else any open session, else none.
 */
export function selectSession({ sessions, pendingId, requestedId, precreated }: SelectionInput): ChatSession | null {
  const available = sessions ?? [];
  const requested = pendingId ?? requestedId;
  if (requested !== null) {
    const match = available.find((session) => session.id === requested);
    if (match) return match;
    if (precreated?.id === requested) return precreated;
  }
  // The server lists newest-first, but a locally prepended row is not in
  // that order yet; sort before picking so "most recent" is explicit.
  const ordered = sortSessionsByActivity(available);
  const withTurns = ordered.find((session) => session.status === "open" && session.last_message_at !== null);
  if (withTurns) return withTurns;
  return ordered.find((session) => session.status === "open") ?? null;
}

/** An address or click names a session the page is deliberately viewing. */
export function isExplicitlyViewing(input: {
  session: Pick<ChatSession, "id"> | null;
  pendingId: string | null;
  requestedId: string | null;
}): boolean {
  if (!input.session) return false;
  return input.session.id === (input.pendingId ?? input.requestedId);
}

export type WarmUpStep =
  | { kind: "skip"; reason: "no-runner" | "runner-cannot-chat" | "viewing-history" | "sessions-pending" }
  | { kind: "warm"; sessionId: string }
  | { kind: "create" };

/**
 * What opening the runner should do (RUN-028). Online and busy runners
 * prepare a session so the first turn streams without a cold start;
 * offline and revoked ones never do. A history session the viewer picked
 * is only warmed when open — a closed one is never replaced by a fresh
 * conversation behind their back. A stale id is not "picked", so
 * auto-create still runs.
 */
export interface WarmUpInput {
  runner: Pick<Runner, "status" | "enrolled_at"> | null | undefined;
  /** False until the history read has answered for the first time. */
  sessionsKnown: boolean;
  session: Pick<ChatSession, "id" | "status"> | null;
  pendingId: string | null;
  requestedId: string | null;
}

export function warmUpStep(input: WarmUpInput): WarmUpStep {
  if (!input.runner) return { kind: "skip", reason: "no-runner" };
  if (!servesChat(input.runner)) return { kind: "skip", reason: "runner-cannot-chat" };
  if (input.session?.status === "open") return { kind: "warm", sessionId: input.session.id };
  if (isExplicitlyViewing(input)) return { kind: "skip", reason: "viewing-history" };
  if (!input.sessionsKnown) return { kind: "skip", reason: "sessions-pending" };
  return { kind: "create" };
}

export interface ChatHeaderSummary {
  name: string;
  /** Working directory, else the pod, else the runner state; may be empty. */
  secondary: string;
  status: RunnerStatus | null;
}

/** What the chat header shows (RUN-032). */
export function chatHeaderSummary(
  runner: Pick<Runner, "name" | "status" | "enrolled_at" | "pod_detail"> | null | undefined,
  session: Pick<ChatSession, "cwd"> | null | undefined
): ChatHeaderSummary {
  if (!runner) return { name: CHAT_RUNNER_FALLBACK_NAME, secondary: "", status: null };
  const status = runnerDisplayStatus(runner);
  const secondary = session?.cwd || runner.pod_detail?.name || status;
  return { name: runner.name, secondary, status };
}
