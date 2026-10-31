// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-437): pricing page, row CLOUD-009 (the
// page states the tiers, the free cap, and answers common questions),
// green on the cloud-edition old app first. The same file must go green
// on apps/web_new once the cloud marketing area lands there.
//
// Observed behavior notes (cloud oracle): the free tier (capped created
// issues, wallet-funded metered usage) contrasts with membership
// (uncapped creation, larger included quotas, external checkout) plus
// an enterprise contact path; the FAQ answers what counts toward the
// cap, what the wallet covers, and that self-hosting is unaffected.
// Assertions stay structural (tier names, prices, link targets, counts)
// and never quote the page copy.
import { test, expect } from "../fixtures";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["CLOUD-009"], "pricing contrasts the tiers with an enterprise path and FAQ"),
  { tag: specTags(["CLOUD-009"]) },
  async ({ driver }) => {
    await driver.openPath("/pricing");
    const tiers = await driver.cloudPricingTiers();
    expect(tiers.map((tier) => tier.name)).toEqual(["Free + pay as you go", "AI Republic membership"]);
    expect(tiers.map((tier) => tier.price)).toEqual(["$0", "$7.99"]);
    expect(tiers.map((tier) => tier.features)).toEqual([5, 5]);
    expect(tiers[0]?.ctaHref).toBe("/login");
    expect(tiers[0]?.ctaExternal).toBe(false);
    expect(tiers[1]?.ctaHref).toBe("https://airepublic.com/pricing");
    expect(tiers[1]?.ctaExternal).toBe(true);
    expect(await driver.cloudPricingEnterpriseHref()).toBe("https://airepublic.com/contact");
    expect(await driver.cloudPricingFaqCount()).toBe(4);
    expect(await driver.cloudPricingFreeCapMentioned()).toBe(true);
  }
);
