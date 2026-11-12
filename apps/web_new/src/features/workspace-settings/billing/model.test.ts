// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Billing catalog rules (parity/specs/workspace-settings.md §29/§30,
// WSET-029/030/032): compared plans and order, frequency amounts and
// discounts, action labels and external targets, and the section titles
// behind the full comparison.
import { describe, expect, it } from "vitest";

import {
  BILLING_COMPARISON,
  BILLING_PLANS,
  comparisonCellLabel,
  findBillingPlan,
  planActionHref,
  planActionLabel,
  planAmount,
  QUOTE_ON_REQUEST,
} from "./model.js";

describe("billing catalog", () => {
  it("compares pro, business and enterprise in that order", () => {
    expect(BILLING_PLANS.map((plan) => plan.name)).toEqual(["Pro", "Business", "Enterprise"]);
    expect(BILLING_PLANS.filter((plan) => plan.popular).map((plan) => plan.name)).toEqual(["Pro"]);
  });

  it("prices pro monthly and yearly with a yearly discount only where it undercuts", () => {
    const pro = findBillingPlan("pro");
    expect(planAmount(pro, "month")).toBe("$8");
    expect(planAmount(pro, "year")).toBe("$6");
    expect(pro.price.discount).toBe("-25%");
    expect(pro.price.qualifier).toBe("per user / month");
  });

  it("quotes business and enterprise without a price or discount on either frequency", () => {
    for (const id of ["business", "enterprise"] as const) {
      const plan = findBillingPlan(id);
      expect(planAmount(plan, "month")).toBeNull();
      expect(planAmount(plan, "year")).toBeNull();
      expect(plan.price.discount).toBeNull();
      expect(plan.price.qualifier).toBe(QUOTE_ON_REQUEST);
    }
  });

  it("labels only the purchasable plan as an upgrade", () => {
    expect(planActionLabel(findBillingPlan("pro"))).toBe("Upgrade to Pro");
    expect(planActionLabel(findBillingPlan("business"))).toBe("Talk to Sales");
    expect(planActionLabel(findBillingPlan("enterprise"))).toBe("Talk to Sales");
  });

  it("targets the per-frequency pricing page for pro and business", () => {
    expect(planActionHref("pro", "month")).toBe("https://airepublic.com/pricing/pro?plan=month");
    expect(planActionHref("pro", "year")).toBe("https://airepublic.com/pricing/pro?plan=year");
    expect(planActionHref("business", "month")).toBe("https://airepublic.com/pricing/business?plan=month");
    expect(planActionHref("business", "year")).toBe("https://airepublic.com/pricing/business?plan=year");
  });

  it("targets the sales contact page for enterprise on both frequencies", () => {
    expect(planActionHref("enterprise", "month")).toBe("https://airepublic.com/contact");
    expect(planActionHref("enterprise", "year")).toBe("https://airepublic.com/contact");
  });

  it("keeps every external target on the product site", () => {
    for (const plan of BILLING_PLANS) {
      for (const frequency of ["month", "year"] as const) {
        expect(planActionHref(plan.id, frequency)).toMatch(/^https:\/\/airepublic\.com\//);
      }
    }
  });

  it("groups the full comparison in several titled sections with one cell per plan", () => {
    expect(BILLING_COMPARISON.map((section) => section.title)).toContain("Project + work tracking");
    expect(BILLING_COMPARISON.length).toBeGreaterThan(1);
    for (const section of BILLING_COMPARISON) {
      for (const feature of section.features) {
        expect(Object.keys(feature.values).sort()).toEqual(["business", "enterprise", "pro"]);
      }
    }
  });

  it("reads flags as included and absent, and passes words through", () => {
    expect(comparisonCellLabel(true)).toBe("Included");
    expect(comparisonCellLabel(false)).toBe("Not included");
    expect(comparisonCellLabel("Unlimited")).toBe("Unlimited");
  });

  it("rejects an unknown plan id", () => {
    // @ts-expect-error — the catalog never carries an unknown id.
    expect(() => findBillingPlan("free")).toThrow(/Unknown billing plan/);
  });
});
