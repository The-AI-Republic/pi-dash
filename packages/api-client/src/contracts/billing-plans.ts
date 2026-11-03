// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace billing page product data (WSET-029-031). On OSS and
// self-managed builds the billing page makes no API call: the plan cards,
// prices and action targets are static. This module pins the part the
// oracle asserts — the external target per plan and frequency, the action
// label, and the price line — so the page and its tests read one source.
// Cloud subscription reads and portal/checkout flows are not observable in
// this checkout (workspace-settings spec §§29-30, 41) and have no contract
// here; the existing account-level plan badge stays in cloud-billing.ts.
import * as z from "zod/mini";

export const BILLING_FREQUENCIES = ["month", "year"] as const;
export type BillingFrequency = (typeof BILLING_FREQUENCIES)[number];

export const BILLING_PLANS = ["Pro", "Business", "Enterprise"] as const;
export type BillingPlan = (typeof BILLING_PLANS)[number];

/**
 * One card's action: the label the button carries and the address it opens
 * in a new tab. Business keeps its sales-flavoured label while landing on
 * its pricing page, which the oracle pins as observed behavior.
 */
export const BillingPlanAction = z.object({
  label: z.string(),
  target: z.string(),
});
export type BillingPlanAction = z.infer<typeof BillingPlanAction>;

const PRO_BASE = "https://airepublic.com/pricing/pro";
const BUSINESS_BASE = "https://airepublic.com/pricing/business";
const SALES_CONTACT = "https://airepublic.com/contact";

/**
 * The action for a plan at the card's selected frequency. Pro and Business
 * open their pricing page with the frequency in `plan`; Enterprise always
 * opens the sales contact page.
 */
export function billingPlanAction(plan: BillingPlan, frequency: BillingFrequency): BillingPlanAction {
  if (plan === "Enterprise") return { label: "Talk to Sales", target: SALES_CONTACT };
  const base = plan === "Pro" ? PRO_BASE : BUSINESS_BASE;
  const label = plan === "Pro" ? "Upgrade to Pro" : "Talk to Sales";
  return { label, target: `${base}?plan=${frequency}` };
}

/**
 * The card's price line. Pro is per user per month and reprices with the
 * frequency; the other plans quote on request at both frequencies, so the
 * switch never changes their line.
 */
export function billingPlanPrice(plan: BillingPlan, frequency: BillingFrequency): string {
  if (plan !== "Pro") return "Quote on request";
  return frequency === "month" ? "$8" : "$6";
}

/** The yearly discount badge, or null where the yearly price does not undercut. */
export function billingPlanDiscount(plan: BillingPlan, frequency: BillingFrequency): string | null {
  if (plan !== "Pro" || frequency !== "year") return null;
  return "-25%";
}

/** Pro is the only plan carrying the popularity marker. */
export function billingPlanIsPopular(plan: BillingPlan): boolean {
  return plan === "Pro";
}
