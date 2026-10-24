// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner status presentation (NEWFRONT-540, RUN-011). Four display states
// share one tone map across the table badge, the rail dot, the detail
// screen and the chat header.

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
 * The state a row displays (RUN-011): the server status, verbatim. The
 * old overview badge, rail dots and chat gate all render raw status —
 * never-enrolled rows read as offline because the server reports them
 * offline, not through a client-side enrolment gate (NEWFRONT-557: the
 * gate contradicted the oracle-pinned chat fixtures, whose live runners
 * carry status online with enrolled_at null).
 */
export function runnerDisplayStatus(runner: Pick<Runner, "status">): RunnerStatus {
  return runner.status;
}
