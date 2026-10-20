// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { MIGRATE_LABEL, UPGRADE_LABEL, accountCenterUrl, isCloudPlan, planLabel, samePlan } from "./plan.js";

function plan(overrides: Record<string, unknown> = {}) {
  return { is_paid: false, migration_required: false, user_center_url: "https://home.test/billing", ...overrides };
}

describe("plan labels", () => {
  it("names a paying account's plan", () => {
    expect(planLabel(plan({ is_paid: true, plan_name: "Team" }))).toBe("Team");
  });

  it("falls back to the upgrade invitation when a paid snapshot has no name", () => {
    expect(planLabel(plan({ is_paid: true }))).toBe(UPGRADE_LABEL);
    expect(planLabel(plan({ is_paid: true, plan_name: "   " }))).toBe(UPGRADE_LABEL);
  });

  it("asks a retired plan to migrate before inviting an upgrade", () => {
    expect(planLabel(plan({ migration_required: true }))).toBe(MIGRATE_LABEL);
    expect(planLabel(plan())).toBe(UPGRADE_LABEL);
  });
});

describe("samePlan", () => {
  it("treats a matching label and destination as unchanged", () => {
    expect(samePlan(plan(), plan({ user_center_url: "https://home.test/billing" }))).toBe(true);
  });

  it("notices a renamed plan or a moved destination", () => {
    expect(samePlan(plan(), plan({ user_center_url: "https://home.test/other" }))).toBe(false);
    expect(samePlan(plan({ is_paid: true, plan_name: "Team" }), plan({ is_paid: true, plan_name: "Scale" }))).toBe(
      false
    );
  });
});

describe("accountCenterUrl", () => {
  it("reads the destination and treats blank or absent as none", () => {
    expect(accountCenterUrl(plan())).toBe("https://home.test/billing");
    expect(accountCenterUrl(plan({ user_center_url: "  " }))).toBeNull();
    expect(accountCenterUrl(null)).toBeNull();
  });
});

describe("isCloudPlan", () => {
  it("recognizes a plan snapshot and rejects everything else", () => {
    expect(isCloudPlan(plan())).toBe(true);
    expect(isCloudPlan(undefined)).toBe(false);
    expect(isCloudPlan(null)).toBe(false);
    expect(isCloudPlan("Team")).toBe(false);
    expect(isCloudPlan({})).toBe(false);
    expect(isCloudPlan({ ...plan(), is_paid: "yes" })).toBe(false);
  });
});
