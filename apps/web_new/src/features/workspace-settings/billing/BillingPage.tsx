// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Billing and plans page body (parity/specs/workspace-settings.md §29–§32,
// WSET-029/030/032). Rendered inside the settings shell, whose gate already
// refuses members and guests. The body is static product data on every
// build: the community summary, the three-plan comparison with per-card
// frequency choices, the highlight row, the expandable full comparison and
// external plan actions. No subscription read, checkout, portal or trial
// surface exists here, and no upgrade dialog exists anywhere.

import * as React from "react";

import { BILLING_PAGE_DESCRIPTION, BILLING_PLANS, COMMUNITY_PLAN, type BillingFrequency } from "./model.js";
import { PlanCard } from "./PlanCard.js";
import { PlanComparison } from "./PlanComparison.js";

const DEFAULT_FREQUENCY: BillingFrequency = "month";

export interface BillingPageProps {
  workspaceSlug: string;
}

export function BillingPage(_props: BillingPageProps): React.ReactElement {
  // One state per plan, so switching one card never reprices another.
  const [frequencies, setFrequencies] = React.useState<Record<string, BillingFrequency>>({});

  const frequencyFor = (planId: string): BillingFrequency => frequencies[planId] ?? DEFAULT_FREQUENCY;
  const setFrequency = (planId: string, frequency: BillingFrequency): void => {
    setFrequencies((current) => ({ ...current, [planId]: frequency }));
  };

  return (
    <section
      aria-label="Billing and plans"
      data-testid="workspace-settings-billing"
      className="flex flex-col gap-(--space-8)"
    >
      <p className="text-body max-w-2xl text-(--text-muted)">{BILLING_PAGE_DESCRIPTION}</p>

      <section
        aria-labelledby="billing-community-heading"
        data-testid="billing-community"
        className="flex flex-col gap-(--space-3) rounded-(--radius-card) border border-(--border) bg-(--surface) p-(--space-4)"
      >
        <h2 id="billing-community-heading" className="text-h3 text-(--text)">
          {COMMUNITY_PLAN.title}
        </h2>
        <p data-testid="billing-community-description" className="text-body text-(--text-muted)">
          {COMMUNITY_PLAN.description}
        </p>
        <ul className="flex flex-wrap gap-x-(--space-4) gap-y-(--space-1)">
          {COMMUNITY_PLAN.points.map((point) => (
            <li key={point} className="text-meta text-(--text)">
              {point}
            </li>
          ))}
        </ul>
      </section>

      <section aria-labelledby="billing-plans-heading" className="flex flex-col gap-(--space-4)">
        <h2 id="billing-plans-heading" className="text-h3 text-(--text)">
          All plans
        </h2>
        <div data-testid="billing-plan-list" className="grid gap-(--space-4) md:grid-cols-3">
          {BILLING_PLANS.map((plan) => (
            <PlanCard
              key={plan.id}
              plan={plan}
              frequency={frequencyFor(plan.id)}
              onFrequencyChange={(frequency) => {
                setFrequency(plan.id, frequency);
              }}
            />
          ))}
        </div>
      </section>

      <PlanComparison />
    </section>
  );
}
