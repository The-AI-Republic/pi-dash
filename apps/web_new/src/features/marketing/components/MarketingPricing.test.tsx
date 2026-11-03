// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pricing page (CLOUD-009). Tier names, prices, destinations, feature
// counts and the FAQ count are what the oracle pins; copy is free.
import { render } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { MarketingPricing } from "./MarketingPricing.js";

describe("MarketingPricing", () => {
  it("owns the page's only top-level heading", () => {
    const { container } = render(<MarketingPricing />);
    expect(container.querySelectorAll("h1")).toHaveLength(1);
  });

  it("contrasts the two tiers with names, prices and five features each", () => {
    const { container } = render(<MarketingPricing />);
    const tiers = [...container.querySelectorAll('[data-testid="pricing-tier"]')];
    expect(tiers).toHaveLength(2);
    expect(tiers.map((tier) => tier.querySelector("p")?.textContent)).toEqual([
      "Free + pay as you go",
      "AI Republic membership",
    ]);
    expect(tiers.map((tier) => tier.querySelector('[data-testid="tier-price"]')?.textContent)).toEqual(["$0", "$7.99"]);
    for (const tier of tiers) {
      expect(tier.querySelectorAll("ul > li")).toHaveLength(5);
    }
  });

  it("sends the free tier to sign-in and membership to the external checkout", () => {
    const { container } = render(<MarketingPricing />);
    const tiers = [...container.querySelectorAll('[data-testid="pricing-tier"]')];
    const freeCta = tiers[0]!.querySelector("a");
    expect(freeCta?.getAttribute("href")).toBe("/login");
    expect(freeCta?.getAttribute("target")).toBeNull();
    const memberCta = tiers[1]!.querySelector("a");
    expect(memberCta?.getAttribute("href")).toBe("https://airepublic.com/pricing");
    expect(memberCta?.getAttribute("target")).toBe("_blank");
  });

  it("names the free creation cap and offers the enterprise path", () => {
    const { container } = render(<MarketingPricing />);
    const tiers = [...container.querySelectorAll('[data-testid="pricing-tier"]')];
    expect(tiers[0]?.textContent).toContain("500");
    const enterprise = container.querySelector('a[href="https://airepublic.com/contact"]');
    expect(enterprise?.getAttribute("target")).toBe("_blank");
  });

  it("answers four questions in the closing FAQ", () => {
    const { container } = render(<MarketingPricing />);
    const all = [...container.querySelectorAll("section")];
    const faq = [...all[all.length - 1]!.querySelectorAll("article")];
    expect(faq).toHaveLength(4);
    expect(faq.map((entry) => entry.querySelector("h3")?.textContent)).toEqual([
      "What counts toward the free cap?",
      "What does the wallet cover?",
      "Do members still pay beyond their quotas?",
      "Are self-hosted deployments affected?",
    ]);
  });
});
