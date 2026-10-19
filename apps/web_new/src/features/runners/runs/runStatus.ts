// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Run-status presentation (NEWFRONT-545, RUN-022). One map serves every
// surface that shows a run's state, including the scheduler-binding detail
// in the schedulers area. Retired or future backend statuses still render:
// the label falls back to the raw token and the tone to neutral.

import type { BadgeTone } from "@pidash/kit";

/** The product's readable state names, keyed by the API status token. */
export const RUN_STATUS_LABELS: Readonly<Record<string, string>> = {
  queued: "queued",
  assigned: "assigned",
  running: "running",
  cancel_requested: "cancellation requested",
  awaiting_approval: "awaiting approval",
  awaiting_reauth: "awaiting reauth",
  paused_awaiting_input: "paused awaiting input",
  blocked: "blocked",
  completed: "completed",
  failed: "failed",
  cancelled: "cancelled",
  refused: "refused",
};

/** Badge tone per status; active states share the warning tone. */
export const RUN_STATUS_TONES: Readonly<Record<string, BadgeTone>> = {
  queued: "neutral",
  assigned: "accent",
  running: "accent",
  cancel_requested: "warning",
  awaiting_approval: "warning",
  awaiting_reauth: "warning",
  paused_awaiting_input: "warning",
  blocked: "warning",
  completed: "success",
  failed: "danger",
  cancelled: "neutral",
  refused: "danger",
};

/** Readable label for a status, or the raw token when it is unknown. */
export function runStatusLabel(status: string): string {
  return RUN_STATUS_LABELS[status] ?? status;
}

/** Tone for a status, neutral for an unknown token. */
export function runStatusTone(status: string): BadgeTone {
  return RUN_STATUS_TONES[status] ?? "neutral";
}
