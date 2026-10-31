// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Agent activity derivation (NEWFRONT-555, RUN-043). There is no server
// agent-state field; the badge is a presentation reading of the live-state
// scalars. Subprocess death and pending approvals outrank freshness, a
// missing or nonsensical event stamp is unknown, and otherwise the badge
// ages from active through thinking to stalled.

import type { RunnerLiveState } from "@pidash/api-client";

export const ACTIVITY_BADGE_KEYS = ["active", "thinking", "stalled", "dead", "awaiting_approval", "unknown"] as const;
export type ActivityBadgeKey = (typeof ACTIVITY_BADGE_KEYS)[number];

export const ACTIVITY_LABELS: Record<ActivityBadgeKey, string> = {
  active: "Active",
  thinking: "Thinking",
  stalled: "Stalled",
  dead: "Subprocess dead",
  awaiting_approval: "Awaiting approval",
  unknown: "Unknown",
};

export const ACTIVITY_TONES: Record<ActivityBadgeKey, "success" | "warning" | "danger" | "accent" | "neutral"> = {
  active: "success",
  thinking: "warning",
  stalled: "danger",
  dead: "danger",
  awaiting_approval: "accent",
  unknown: "neutral",
};

/** Last activity within this window reads active. */
export const ACTIVE_WINDOW_MS = 30_000;
/** Within this window after that reads thinking; past it, stalled. */
export const THINKING_WINDOW_MS = 180_000;
/** The panel re-derives on this cadence without a server fetch. */
export const ACTIVITY_TICK_MS = 5_000;

/** Derive the badge from the live snapshot and a "now" millisecond stamp. */
export function activityBadge(state: RunnerLiveState | null | undefined, now: number): ActivityBadgeKey {
  if (state === null || state === undefined) return "unknown";
  if (state.agent_subprocess_alive === false) return "dead";
  if ((state.approvals_pending ?? 0) > 0) return "awaiting_approval";
  if (state.last_event_at === null) return "unknown";
  const at = Date.parse(state.last_event_at);
  if (Number.isNaN(at)) return "unknown";
  const age = now - at;
  if (age < 0) return "unknown";
  if (age <= ACTIVE_WINDOW_MS) return "active";
  if (age <= THINKING_WINDOW_MS) return "thinking";
  return "stalled";
}

/** Compact "Ns ago" / "Nm ago" / "Nh ago", or a dash when the stamp is unusable. */
export function relativeAge(iso: string | null, now: number): string {
  if (iso === null) return "—";
  const at = Date.parse(iso);
  if (Number.isNaN(at)) return "—";
  const age = now - at;
  if (age < 0) return "—";
  const seconds = Math.round(age / 1000);
  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.round(seconds / 60)}m ago`;
  return `${Math.round(seconds / 3600)}h ago`;
}

/** Thousands become k, millions become M, both with one decimal. */
export function compactCount(value: number | null | undefined): string {
  if (value === null || value === undefined) return "—";
  if (value < 1_000) return String(value);
  if (value < 1_000_000) return `${(value / 1_000).toFixed(1)}k`;
  return `${(value / 1_000_000).toFixed(1)}M`;
}

/** Three-way liveness label; null means the runner never reported it. */
export function livenessLabel(value: boolean | null | undefined): string {
  if (value === null || value === undefined) return "—";
  return value ? "yes" : "no";
}
