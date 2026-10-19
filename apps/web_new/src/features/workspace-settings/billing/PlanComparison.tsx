// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Full feature comparison (parity/specs/workspace-settings.md §29,
// WSET-029). Collapsed by default behind one control; expanded it lists
// each titled section in order with one cell per compared plan. Static
// product data, so nothing here reads or writes the server.

import { Check, Minus } from "lucide-react";
import * as React from "react";

import {
  BILLING_COMPARISON,
  BILLING_PLANS,
  comparisonCellLabel,
  type ComparisonFeature,
  type PlanId,
} from "./model.js";

export const COMPARE_EXPAND_LABEL = "Compare all features";
export const COMPARE_COLLAPSE_LABEL = "Collapse comparison";

function FeatureCell({ feature, planId }: { feature: ComparisonFeature; planId: PlanId }): React.ReactElement {
  const value = feature.values[planId];
  if (typeof value === "string") {
    return <span className="text-meta text-(--text)">{comparisonCellLabel(value)}</span>;
  }
  return value ? (
    <span className="text-(--success)" title={comparisonCellLabel(true)}>
      <Check size={16} strokeWidth={1.5} aria-hidden="true" />
      <span className="sr-only">{comparisonCellLabel(true)}</span>
    </span>
  ) : (
    <span className="text-(--text-muted)" title={comparisonCellLabel(false)}>
      <Minus size={16} strokeWidth={1.5} aria-hidden="true" />
      <span className="sr-only">{comparisonCellLabel(false)}</span>
    </span>
  );
}

export function PlanComparison(): React.ReactElement {
  const [expanded, setExpanded] = React.useState(false);
  return (
    <section aria-label="All plans" data-testid="billing-comparison" className="flex flex-col gap-(--space-3)">
      <button
        type="button"
        aria-expanded={expanded}
        data-testid="billing-compare-toggle"
        onClick={() => {
          setExpanded((current) => !current);
        }}
        className="text-body w-fit cursor-pointer rounded-(--radius-control) border border-(--border) px-(--space-4) py-(--space-2) text-(--text) hover:bg-(--subtle)"
      >
        {expanded ? COMPARE_COLLAPSE_LABEL : COMPARE_EXPAND_LABEL}
      </button>

      {expanded ? (
        <div className="flex flex-col gap-(--space-6)">
          {BILLING_COMPARISON.map((section) => (
            <div
              key={section.id}
              data-testid={`billing-comparison-section-${section.id}`}
              className="flex flex-col gap-(--space-2)"
            >
              <h3 className="text-h3 text-(--text)">{section.title}</h3>
              <table className="w-full border-collapse text-left">
                <thead>
                  <tr className="border-b border-(--border)">
                    <th scope="col" className="text-caption py-(--space-2) font-medium text-(--text-muted)">
                      Feature
                    </th>
                    {BILLING_PLANS.map((plan) => (
                      <th
                        key={plan.id}
                        scope="col"
                        className="text-caption py-(--space-2) font-medium text-(--text-muted)"
                      >
                        {plan.name}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {section.features.map((feature) => (
                    <tr key={feature.name} className="border-b border-(--border)">
                      <th scope="row" className="text-meta font-normal py-(--space-2) text-(--text)">
                        {feature.name}
                      </th>
                      {BILLING_PLANS.map((plan) => (
                        <td key={plan.id} className="py-(--space-2)">
                          <FeatureCell feature={feature} planId={plan.id} />
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ))}
        </div>
      ) : null}
    </section>
  );
}
