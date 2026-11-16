// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collaboration reconnect machine (parity/specs/pages.md §8.4, PAGE-067).
// An explicit walk from the first dial through connecting, awaiting-sync
// and synced, with counted reconnect attempts into a terminal
// disconnected state carrying a typed reason. The session core owns the
// timers; this module owns the transitions, the badge mapping and the
// backoff, all pure.

import type { PageSyncStatus } from "./pageSync.js";

/** Why the session gave up: rejected credentials, a forced close, or spent retries. */
export type PageLiveDisconnectReason = "auth" | "force-close" | "exhausted";

export type PageLiveStage =
  | { kind: "initial" }
  | { kind: "connecting" }
  | { kind: "awaiting-sync" }
  | { kind: "synced" }
  | { kind: "reconnecting"; attempt: number }
  | { kind: "disconnected"; reason: PageLiveDisconnectReason };

export type PageLiveStageEvent =
  | { type: "start" }
  | { type: "opened" }
  | { type: "synced" }
  | { type: "dropped"; attempt: number }
  | { type: "failed"; reason: PageLiveDisconnectReason }
  | { type: "reset" };

function live(stage: PageLiveStage): boolean {
  return (
    stage.kind === "connecting" ||
    stage.kind === "awaiting-sync" ||
    stage.kind === "synced" ||
    stage.kind === "reconnecting"
  );
}

/**
 * One transition. Stray events (a late sync after a reset, a drop once
 * disconnected) leave the stage untouched, so out-of-order socket
 * callbacks can never corrupt the walk.
 */
export function nextPageLiveStage(stage: PageLiveStage, event: PageLiveStageEvent): PageLiveStage {
  switch (event.type) {
    case "reset":
      return { kind: "initial" };
    case "start":
      if (stage.kind === "initial" || stage.kind === "disconnected") return { kind: "connecting" };
      return stage;
    case "opened":
      if (stage.kind === "connecting" || stage.kind === "reconnecting") return { kind: "awaiting-sync" };
      return stage;
    case "synced":
      if (live(stage)) return { kind: "synced" };
      return stage;
    case "dropped":
      if (live(stage)) return { kind: "reconnecting", attempt: event.attempt };
      return stage;
    case "failed":
      if (live(stage)) return { kind: "disconnected", reason: event.reason };
      return stage;
  }
}

/** The header word each stage reports through the sync seam (PAGE-066). */
export function pageLiveSyncReport(stage: PageLiveStage): PageSyncStatus {
  if (stage.kind === "synced") return "synced";
  if (stage.kind === "disconnected") return "lost";
  return "syncing";
}

/** Retries after the first dial before the session goes terminal. */
export const PAGE_LIVE_MAX_ATTEMPTS = 8;

/** The first retry waits this long; each attempt doubles it. */
export const PAGE_LIVE_RETRY_BASE_MS = 1000;

/** No retry waits longer than this beat. */
export const PAGE_LIVE_RETRY_CAP_MS = 10_000;

/** One attempt that never opens is dropped after this beat. */
export const PAGE_LIVE_CONNECT_TIMEOUT_MS = 10_000;

/** The backoff beat before retry `attempt` (1-based): doubling, capped. */
export function pageLiveRetryDelayMs(attempt: number): number {
  return Math.min(PAGE_LIVE_RETRY_BASE_MS * 2 ** (attempt - 1), PAGE_LIVE_RETRY_CAP_MS);
}
