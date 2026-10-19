// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Billing and plans catalog (parity/specs/workspace-settings.md §29–§32,
// WSET-029/030/032). Static product data only: on every build the page
// reads no endpoint, and each plan action leaves for an external page.
// The catalog names the three compared plans in display order, the
// per-frequency price lines, the popularity marker and the external
// targets. Pure: the page and its tests share one answer.

export type BillingFrequency = "month" | "year";

export type PlanId = "pro" | "business" | "enterprise";

export interface PlanPrice {
  /** Amount shown on monthly, or null when the plan is quoted. */
  monthly: string | null;
  /** Amount shown on yearly, or null when the plan is quoted. */
  yearly: string | null;
  /** Badge shown beside a price, or null when yearly does not undercut. */
  discount: string | null;
  /** Text after the amount; the quote line uses it alone. */
  qualifier: string;
}

export interface PlanDefinition {
  id: PlanId;
  name: string;
  /** Whether the plan carries the popularity marker. */
  popular: boolean;
  /** Whether the action reads as a purchase (the rest contact sales). */
  purchasable: boolean;
  price: PlanPrice;
  /** Headline capabilities listed in the always-visible highlight row. */
  highlights: readonly string[];
}

export interface ComparisonFeature {
  name: string;
  /** One cell per plan id; booleans render as included/absent. */
  values: Record<PlanId, boolean | string>;
}

export interface ComparisonSection {
  id: string;
  title: string;
  features: readonly ComparisonFeature[];
}

/** Community summary copy (§29): the plan every build starts on. */
export const COMMUNITY_PLAN = {
  title: "Community",
  description: "Unlimited projects, work items, cycles, modules, pages and storage.",
  points: ["No limits on core project work", "Community-driven support"],
} as const;

export const BILLING_PAGE_DESCRIPTION =
  "Compare what each plan includes and choose the one that fits how the workspace works.";

/** Compared plans and their order; the free and one-person plans never render. */
export const BILLING_PLANS: readonly PlanDefinition[] = [
  {
    id: "pro",
    name: "Pro",
    popular: true,
    purchasable: true,
    price: { monthly: "$8", yearly: "$6", discount: "-25%", qualifier: "per user / month" },
    highlights: ["Unlimited teammates", "Custom work item types", "Reusable work item templates", "Full time tracking"],
  },
  {
    id: "business",
    name: "Business",
    popular: false,
    purchasable: false,
    price: { monthly: null, yearly: null, discount: null, qualifier: "Quote on request" },
    highlights: ["Role-based access", "Reusable project setups", "Baseline tracking", "Custom reports"],
  },
  {
    id: "enterprise",
    name: "Enterprise",
    popular: false,
    purchasable: false,
    price: { monthly: null, yearly: null, discount: null, qualifier: "Quote on request" },
    highlights: [
      "Private and managed deployments",
      "Directory sign-in",
      "Managed access control",
      "Formulas and databases",
    ],
  },
];

export const QUOTE_ON_REQUEST = "Quote on request";

export function findBillingPlan(id: PlanId): PlanDefinition {
  const plan = BILLING_PLANS.find((entry) => entry.id === id);
  if (plan === undefined) throw new Error(`Unknown billing plan: ${id}`);
  return plan;
}

/** Amount line for a frequency; quote plans keep their single quote line. */
export function planAmount(plan: PlanDefinition, frequency: BillingFrequency): string | null {
  return frequency === "month" ? plan.price.monthly : plan.price.yearly;
}

/** Card action label: purchase plans invite an upgrade, the rest a conversation. */
export function planActionLabel(plan: PlanDefinition): string {
  return plan.purchasable ? `Upgrade to ${plan.name}` : "Talk to Sales";
}

/**
 * External target for a plan action (§30). Business keeps its pricing page
 * even though its label asks for sales; Enterprise pairs both frequencies
 * with the sales contact page.
 */
export function planActionHref(id: PlanId, frequency: BillingFrequency): string {
  if (id === "enterprise") return "https://airepublic.com/contact";
  return `https://airepublic.com/pricing/${id}?plan=${frequency}`;
}

/** Feature comparison behind the expander, grouped in titled sections. */
export const BILLING_COMPARISON: readonly ComparisonSection[] = [
  {
    id: "work",
    title: "Project + work tracking",
    features: [
      {
        name: "Projects and work items",
        values: { pro: "Unlimited", business: "Unlimited", enterprise: "Unlimited" },
      },
      { name: "Cycles and modules", values: { pro: true, business: true, enterprise: true } },
      { name: "Work item templates", values: { pro: true, business: true, enterprise: true } },
      { name: "Time tracking", values: { pro: "Full", business: "Full", enterprise: "Full" } },
    ],
  },
  {
    id: "access",
    title: "Teams and access",
    features: [
      { name: "Unlimited teammates", values: { pro: true, business: true, enterprise: true } },
      { name: "Role-based access", values: { pro: false, business: true, enterprise: true } },
      { name: "Guests", values: { pro: false, business: true, enterprise: true } },
      { name: "Directory sign-in", values: { pro: false, business: false, enterprise: true } },
    ],
  },
  {
    id: "administration",
    title: "Administration and controls",
    features: [
      { name: "Baseline tracking", values: { pro: false, business: true, enterprise: true } },
      { name: "Custom reports", values: { pro: false, business: true, enterprise: true } },
      { name: "Managed deployment", values: { pro: false, business: false, enterprise: true } },
      { name: "Formulas and databases", values: { pro: false, business: false, enterprise: true } },
    ],
  },
];

/** Cell text for a comparison value: words pass through, flags read as included/absent. */
export function comparisonCellLabel(value: boolean | string): string {
  if (value === true) return "Included";
  if (value === false) return "Not included";
  return value;
}
