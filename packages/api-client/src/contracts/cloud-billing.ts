// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud billing contracts: the plan snapshot behind the workspace plan
// badge (CLOUD-038). Shapes follow parity/specs/cloud.md and the CLOUD
// oracle rows, not old code. The cloud backend serves this path; the OSS
// stack answers 404 (the contract suite pins the wiring there, and the
// area gate proves the cloud positive).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";

/**
 * The plan snapshot: the paid flag, the legacy-migration flag, and the
 * external account-center address (all oracle-pinned), plus the paid
 * plan label the badge shows for paying accounts. The label's field
 * name follows the spec's prose — the oracle pins only the free-account
 * shape, so it stays optional until the area gate proves it on the
 * cloud stack; otherwise the badge falls back to the upgrade label.
 * The snapshot carries more (payment state, entitlement limits); the
 * badge reads these, so the rest stays out of the contract.
 */
export const CloudBillingPlan = z.object({
  is_paid: z.boolean(),
  migration_required: z.boolean(),
  user_center_url: z.string(),
  plan_name: z.optional(z.string()),
});
export type CloudBillingPlan = z.infer<typeof CloudBillingPlan>;

/**
 * GET /api/billing/me/plan/ reads the caller's plan snapshot. The
 * badge maps it to a label: the paid name for paying accounts, a
 * migrate prompt for legacy pre-cutover plans, an upgrade label
 * otherwise — opening the account-center address in a new tab.
 */
export async function getCloudBillingPlan(client: ApiClient): Promise<CloudBillingPlan> {
  const body = await client.getJson<unknown>("/api/billing/me/plan/");
  return client.parse(CloudBillingPlan, body, "GET /api/billing/me/plan/");
}
