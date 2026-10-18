// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner status badge and rail dot (NEWFRONT-540, RUN-011). The two read
// the same tone map so the table column and the side rail can never
// disagree about a runner's state.

import { Badge, type BadgeTone } from "@pidash/kit";
import * as React from "react";

import { RUNNER_STATUS_LABELS, RUNNER_STATUS_TONES, runnerDisplayStatus, type RunnerStatus } from "../status.js";

/** Dot colour per tone; the badge renders its own dot from the same tone. */
const DOT_TEXT: Record<BadgeTone, string> = {
  neutral: "text-(--text-muted)",
  accent: "text-(--accent)",
  success: "text-(--success)",
  warning: "text-(--warning)",
  danger: "text-(--danger)",
};

export interface RunnerStatusBadgeProps {
  status: RunnerStatus;
}

export function RunnerStatusBadge({ status }: RunnerStatusBadgeProps): React.ReactElement {
  return (
    <Badge tone={RUNNER_STATUS_TONES[status]} dot>
      {RUNNER_STATUS_LABELS[status]}
    </Badge>
  );
}

export interface RunnerStatusDotProps {
  status: RunnerStatus;
}

/**
 * Decorative dot beside a rail contact; the contact name is the label.
 * Drawn as an SVG circle so the contact reads as "glyph + status mark"
 * to anything inspecting its icons.
 */
export function RunnerStatusDot({ status }: RunnerStatusDotProps): React.ReactElement {
  return (
    <svg
      aria-hidden="true"
      data-status={status}
      viewBox="0 0 8 8"
      className={`size-2 shrink-0 ${DOT_TEXT[RUNNER_STATUS_TONES[status]]}`}
    >
      <circle cx="4" cy="4" r="4" fill="currentColor" />
    </svg>
  );
}

/** Convenience for callers holding a raw runner row. */
export function runnerStatusOf(runner: Parameters<typeof runnerDisplayStatus>[0]): RunnerStatus {
  return runnerDisplayStatus(runner);
}
