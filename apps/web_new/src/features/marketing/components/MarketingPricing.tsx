// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pricing page (CLOUD-009, parity/specs/cloud.md §2): the free tier
// against membership, the enterprise path and the FAQ. Tier names,
// prices, destinations and counts are what the oracle pins; the rest of
// the copy is ours.
import * as React from "react";

import { MarketingColumn } from "./MarketingFrame.js";

import { ENTERPRISE_CONTACT_URL, metaFor, PRICING_FAQ, PRICING_TIERS, type PricingTier } from "../content.js";

const PRIMARY_ACTION =
  "text-body inline-flex items-center rounded-(--radius-control) bg-(--accent) px-(--space-6) py-(--space-3) font-medium text-(--on-accent) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:outline-none";
const SECONDARY_ACTION =
  "text-body inline-flex items-center rounded-(--radius-control) border border-(--border) px-(--space-6) py-(--space-3) font-medium text-(--text) hover:bg-(--subtle) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-2 focus-visible:outline-none";

export function MarketingPricing(): React.ReactElement {
  return (
    <MarketingColumn className="py-(--space-8)">
      <PricingHero />
      <TiersSection />
      <EnterpriseSection />
      <FaqSection />
    </MarketingColumn>
  );
}

function PricingHero(): React.ReactElement {
  return (
    <section className="flex flex-col gap-(--space-4) py-(--space-8)">
      <h1 className="text-h1 font-semibold text-balance text-(--text)">Pricing.</h1>
      <p className="text-body max-w-2xl text-(--text-muted)">{metaFor("/pricing").description}</p>
    </section>
  );
}

function SectionHeading({ id, children }: { id: string; children: React.ReactNode }): React.ReactElement {
  return (
    <h2 id={id} className="text-h2 font-semibold text-(--text)">
      {children}
    </h2>
  );
}

function TiersSection(): React.ReactElement {
  return (
    <section aria-labelledby="pricing-tiers-heading" className="py-(--space-10)">
      <div className="flex flex-col gap-(--space-6)">
        <SectionHeading id="pricing-tiers-heading">Two ways to run</SectionHeading>
        <div className="grid gap-(--space-6) md:grid-cols-2">
          {PRICING_TIERS.map((tier) => (
            <TierCard key={tier.name} tier={tier} />
          ))}
        </div>
      </div>
    </section>
  );
}

function TierCard({ tier }: { tier: PricingTier }): React.ReactElement {
  // The name stays the first paragraph and the price its own test id:
  // the parity read takes both positionally.
  return (
    <article
      data-testid="pricing-tier"
      className="flex flex-col gap-(--space-4) rounded-(--radius-card) border border-(--border) p-(--space-8)"
    >
      <p className="text-body font-medium text-(--text)">{tier.name}</p>
      <p className="flex items-baseline gap-(--space-2)">
        <span data-testid="tier-price" className="text-h1 font-semibold text-(--text)">
          {tier.price}
        </span>
        <span className="text-caption text-(--text-muted)">{tier.billing}</span>
      </p>
      <ul className="flex flex-col gap-(--space-2)">
        {tier.features.map((feature) => (
          <li key={feature} className="text-body text-(--text-muted)">
            {feature}
          </li>
        ))}
      </ul>
      <a
        href={tier.href}
        {...(tier.external === true ? { target: "_blank", rel: "noreferrer noopener" } : {})}
        className={tier.external === true ? PRIMARY_ACTION : SECONDARY_ACTION}
      >
        {tier.label}
      </a>
    </article>
  );
}

function EnterpriseSection(): React.ReactElement {
  return (
    <section aria-labelledby="pricing-enterprise-heading" className="py-(--space-10)">
      <div className="flex flex-col gap-(--space-4) rounded-(--radius-card) border border-(--border) p-(--space-8) md:flex-row md:items-center md:justify-between">
        <div className="flex flex-col gap-(--space-2)">
          <SectionHeading id="pricing-enterprise-heading">Running an organization?</SectionHeading>
          <p className="text-body max-w-xl text-(--text-muted)">
            Organization billing and tailored deployments through our team.
          </p>
        </div>
        <a
          href={ENTERPRISE_CONTACT_URL}
          target="_blank"
          rel="noreferrer noopener"
          className={`${SECONDARY_ACTION} shrink-0`}
        >
          Contact us
        </a>
      </div>
    </section>
  );
}

function FaqSection(): React.ReactElement {
  return (
    <section aria-labelledby="pricing-faq-heading" className="py-(--space-10)">
      <div className="flex flex-col gap-(--space-6)">
        <SectionHeading id="pricing-faq-heading">Questions</SectionHeading>
        <div className="grid gap-(--space-6) md:grid-cols-2">
          {PRICING_FAQ.map((entry) => (
            <article
              key={entry.question}
              data-testid="pricing-faq"
              className="flex flex-col gap-(--space-2) rounded-(--radius-card) bg-(--subtle) p-(--space-6)"
            >
              <h3 className="text-body font-medium text-(--text)">{entry.question}</h3>
              <p className="text-caption text-(--text-muted)">{entry.answer}</p>
            </article>
          ))}
        </div>
      </div>
    </section>
  );
}
