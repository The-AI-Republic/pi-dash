// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace plan badge (parity/specs/cloud.md §8, CLOUD-038; intended
// behavior for bug NEWFRONT-474). The badge names the account's plan and
// opens the external account center in a new tab; its hover hint carries
// the running app version. The plan reads on mount and again on window
// focus, and a refresh failure leaves the last known label in place.
// Without a destination the badge still labels the plan but is inert.

import * as React from "react";

import { edition } from "../../../core/edition/index.js";
import { platform } from "../../../core/platform/index.js";
import { useCloudPlan } from "../api/queries.js";
import { accountCenterUrl, isCloudPlan, planLabel } from "../lib/plan.js";

export const PLAN_BADGE_TESTID = "cloud-plan-badge";
export const PLAN_BADGE_LABEL = "Plan";

/**
 * The hint the badge carries: the running version, so a support
 * conversation can start from what the visitor actually runs.
 */
export function planBadgeHint(version: string): string {
  return `Version: v${version}`;
}

export interface PlanBadgeProps {
  /**
   * The running app version for the hover hint. The shell reads it once
   * and passes it down: the badge itself stays out of the initial chain,
   * and importing the version there would pull the shell chunk back in.
   */
  version: string;
}

export function PlanBadge({ version }: PlanBadgeProps): React.ReactElement | null {
  // Only editions that publish a plan snapshot show the badge. The lazy
  // chunk is shared, so the gate lives here and the read stays idle.
  if (edition.flags["plan-badge"] !== true) return null;
  const plan = useCloudPlan(true);
  // Nothing has answered yet: rendering a made-up label would be a claim
  // about the account the badge cannot back up.
  if (!isCloudPlan(plan.data)) return null;
  const label = planLabel(plan.data);
  const destination = accountCenterUrl(plan.data);
  const open = (): void => {
    if (destination === null) return;
    void platform.openExternal(destination);
  };

  // The native title carries the version hint: it reaches mouse and
  // assistive users on every platform and keeps the shell's first paint
  // free of a tooltip runtime (size gate).
  return (
    <button
      type="button"
      data-testid={PLAN_BADGE_TESTID}
      aria-label={`${PLAN_BADGE_LABEL}: ${label}`}
      aria-disabled={destination === null || undefined}
      title={planBadgeHint(version)}
      className="text-caption w-full truncate rounded-(--radius-full) bg-(--accent-subtle) px-(--space-2) py-(--space-1) text-left font-medium text-(--accent) enabled:hover:brightness-95 aria-disabled:cursor-default"
      onClick={open}
    >
      {label}
    </button>
  );
}
