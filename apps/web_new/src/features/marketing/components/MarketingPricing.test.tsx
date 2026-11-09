// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pricing page (CLOUD-009). Tier names, prices, destinations, feature
// counts and the FAQ count are what the oracle pins; copy is free.
import { render } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const editionHarness = vi.hoisted(() => ({ flags: {} as Record<string, boolean> }));
vi.mock("../../../core/edition/index.js", () => ({
  PRICING_FREE_TOKENS_FLAG: "pricing-free-tokens",
  get edition() {
    return { id: "test", flags: editionHarness.flags };
  },
}));

import { PRICING_FREE_TOKENS_FLAG } from "../../../core/edition/index.js";
import { MarketingPricing } from "./MarketingPricing.js";

afterEach(() => {
  editionHarness.flags = {};
});

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

  it("hides the free-token block while the edition flag is absent (OSS)", () => {
    const { container } = render(<MarketingPricing />);
    expect(container.querySelector('[data-testid="pricing-free-tokens"]')).toBeNull();
    expect(container.textContent).not.toContain("2M");
  });

  it("shows the daily allowance block when the edition flag is set (cloud)", () => {
    editionHarness.flags = { [PRICING_FREE_TOKENS_FLAG]: true };
    const { container } = render(<MarketingPricing />);
    const block = container.querySelector('[data-testid="pricing-free-tokens"]');
    expect(block?.textContent).toContain("2M");
    expect(block?.textContent).toContain("5M");
    expect(block?.textContent).toContain("deepseek-v4-flash");
    const section = container.querySelector('section[aria-label="Free daily LLM tokens"]');
    expect(section).not.toBeNull();
    expect(section?.querySelector("#pricing-free-tokens-heading")?.textContent).toContain("Free tokens");
    // The block sits between tiers and FAQ: it is not a tier nor an FAQ entry.
    expect(container.querySelectorAll('[data-testid="pricing-tier"]')).toHaveLength(2);
    expect(container.querySelectorAll('[data-testid="pricing-faq"]')).toHaveLength(4);
    const sections = [...container.querySelectorAll("section")];
    const faqIdx = sections.findIndex((s) => s.querySelector('[data-testid="pricing-faq"]'));
    const tokenIdx = sections.findIndex((s) => s.getAttribute("aria-label") === "Free daily LLM tokens");
    expect(tokenIdx).toBeGreaterThan(-1);
    expect(tokenIdx).toBeLessThan(faqIdx);
    expect(sections[sections.length - 1]!.querySelectorAll('[data-testid="pricing-faq"]')).toHaveLength(4);
  });

  it("keeps the reset and wallet fallback copy inside the cloud block", () => {
    editionHarness.flags = { [PRICING_FREE_TOKENS_FLAG]: true };
    const { container } = render(<MarketingPricing />);
    const section = container.querySelector('section[aria-label="Free daily LLM tokens"]');
    expect(section?.textContent).toMatch(/00:00 UTC/);
    expect(section?.textContent).toMatch(/do not carry over/i);
    expect(section?.textContent).toMatch(/OpenHub credit/i);
    expect(section?.textContent).toMatch(/wallet/i);
  });
});
