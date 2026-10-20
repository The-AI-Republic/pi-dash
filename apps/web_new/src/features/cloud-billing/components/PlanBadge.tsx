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

import { APP_VERSION } from "../../../shared/shell/version.js";
import { useCloudPlan } from "../api/queries.js";
import { accountCenterUrl, isCloudPlan, planLabel } from "../lib/plan.js";

export const PLAN_BADGE_TESTID = "cloud-plan-badge";
export const PLAN_BADGE_LABEL = "Plan";

/**
 * The hint the badge carries: the running version, so a support
 * conversation can start from what the visitor actually runs.
 */
export function planBadgeHint(version: string = APP_VERSION): string {
  return `Version: v${version}`;
}

export interface PlanBadgeProps {
  /**
   * Opens an external address. The shell supplies the platform opener so
   * the desktop build uses its own window; the default is the browser's.
   */
  onOpenExternal?: ((url: string) => void) | undefined;
  /** A fixed version for tests and stories; the app version otherwise. */
  version?: string | undefined;
}

export function PlanBadge({ onOpenExternal, version }: PlanBadgeProps): React.ReactElement | null {
  const plan = useCloudPlan(true);
  // Nothing has answered yet: rendering a made-up label would be a claim
  // about the account the badge cannot back up.
  if (!isCloudPlan(plan.data)) return null;
  const label = planLabel(plan.data);
  const destination = accountCenterUrl(plan.data);
  const open = (): void => {
    if (destination === null) return;
    if (onOpenExternal !== undefined) onOpenExternal(destination);
    else globalThis.window?.open(destination, "_blank", "noopener,noreferrer");
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
      className="flex w-full items-center justify-between gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) py-(--space-1) text-left text-(--text) enabled:hover:bg-(--subtle) aria-disabled:cursor-default aria-disabled:opacity-70"
      onClick={open}
    >
      <span className="text-caption truncate rounded-(--radius-full) bg-(--accent-subtle) px-(--space-2) py-(--space-0) font-medium text-(--accent)">
        {label}
      </span>
    </button>
  );
}
