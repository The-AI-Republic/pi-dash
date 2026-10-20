// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Plan labels for the workspace badge (parity/specs/cloud.md §8,
// CLOUD-038). A paying account reads its plan by name; a retired
// pre-cutover plan is told to migrate rather than buy again; everyone
// else is invited to upgrade. The label is derived only from the answer,
// so a rename of a plan on the account home shows up without a release.
import type { CloudBillingPlan } from "@pidash/api-client";

export const UPGRADE_LABEL = "Upgrade";
export const MIGRATE_LABEL = "Migrate plan";

/** True for a plan snapshot structural sharing may compare. */
export function isCloudPlan(value: unknown): value is CloudBillingPlan {
  if (typeof value !== "object" || value === null) return false;
  const plan = value as Partial<CloudBillingPlan>;
  return (
    typeof plan.is_paid === "boolean" &&
    typeof plan.migration_required === "boolean" &&
    typeof plan.user_center_url === "string"
  );
}

/** The badge's visible label for one plan snapshot. */
export function planLabel(plan: CloudBillingPlan): string {
  if (plan.is_paid) {
    return plan.plan_name !== undefined && plan.plan_name.trim() !== "" ? plan.plan_name : UPGRADE_LABEL;
  }
  return plan.migration_required ? MIGRATE_LABEL : UPGRADE_LABEL;
}

/**
 * True when two snapshots show the same badge. Only the label and the
 * destination matter; the rest of the snapshot (entitlements, payment
 * state) changes nothing on screen, so a matching answer skips the
 * state write and the re-render it would cause.
 */
export function samePlan(a: CloudBillingPlan, b: CloudBillingPlan): boolean {
  return planLabel(a) === planLabel(b) && a.user_center_url === b.user_center_url;
}

/** The account-center address, or null when the answer carries none. */
export function accountCenterUrl(plan: CloudBillingPlan | null): string | null {
  const url = plan?.user_center_url.trim() ?? "";
  return url === "" ? null : url;
}
