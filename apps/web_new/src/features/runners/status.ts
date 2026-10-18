// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner status presentation (NEWFRONT-540, RUN-011). Four display states
// share one tone map across the table badge and the rail dot. A row that
// was added but never enrolled has no live session yet and reads as
// offline until its first token exchange.

import type { BadgeTone } from "@pidash/kit";
import type { Runner } from "@pidash/api-client";

export const RUNNER_STATUSES = ["online", "busy", "offline", "revoked"] as const;
export type RunnerStatus = (typeof RUNNER_STATUSES)[number];

/** The four states, label first. Order follows the status vocabulary. */
export const RUNNER_STATUS_LABELS: Record<RunnerStatus, string> = {
  online: "online",
  busy: "busy",
  offline: "offline",
  revoked: "revoked",
};

/** Badge tone per state; the rail dot reuses the same tone. */
export const RUNNER_STATUS_TONES: Record<RunnerStatus, BadgeTone> = {
  online: "success",
  busy: "accent",
  offline: "neutral",
  revoked: "warning",
};

/**
 * The state a row displays (RUN-011). A never-enrolled runner reads
 * offline; everything else reports its server status.
 */
export function runnerDisplayStatus(runner: Pick<Runner, "status" | "enrolled_at">): RunnerStatus {
  if (runner.enrolled_at === null) return "offline";
  return runner.status;
}
