// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One plan card (parity/specs/workspace-settings.md §29/§30, WSET-029/030).
// The card owns its own month/year choice, so cards reprice independently;
// its action opens the external pricing or sales page in a new tab and
// leaves the page untouched. Presentational: the page holds the state.

import { Badge } from "@pidash/kit";
import { ArrowUpRight } from "lucide-react";
import * as React from "react";

import {
  planActionHref,
  planActionLabel,
  planAmount,
  QUOTE_ON_REQUEST,
  type BillingFrequency,
  type PlanDefinition,
} from "./model.js";

export interface PlanCardProps {
  plan: PlanDefinition;
  frequency: BillingFrequency;
  onFrequencyChange: (frequency: BillingFrequency) => void;
}

const FREQUENCIES: ReadonlyArray<{ id: BillingFrequency; label: string }> = [
  { id: "month", label: "Monthly" },
  { id: "year", label: "Yearly" },
];

export function PlanCard({ plan, frequency, onFrequencyChange }: PlanCardProps): React.ReactElement {
  const amount = planAmount(plan, frequency);
  const href = planActionHref(plan.id, frequency);

  return (
    <article
      aria-label={`${plan.name} plan`}
      data-testid={`billing-plan-${plan.id}`}
      data-frequency={frequency}
      className="flex min-w-0 flex-col gap-(--space-3) rounded-(--radius-card) border border-(--border) bg-(--bg) p-(--space-4)"
    >
      <header className="flex items-center gap-(--space-2)">
        <h3 className="text-emphasis text-(--text)">{plan.name}</h3>
        {plan.popular ? <Badge tone="accent">Popular</Badge> : null}
      </header>

      <div
        aria-label={`${plan.name} billing frequency`}
        role="group"
        className="flex w-fit items-center rounded-(--radius-control) border border-(--border) p-px"
      >
        {FREQUENCIES.map((option) => {
          const selected = option.id === frequency;
          return (
            <button
              key={option.id}
              type="button"
              aria-pressed={selected}
              data-testid={`billing-frequency-${plan.id}-${option.id}`}
              onClick={() => {
                onFrequencyChange(option.id);
              }}
              className={`text-meta cursor-pointer rounded-(--radius-control) px-(--space-3) py-1 ${
                selected ? "bg-(--subtle) font-medium text-(--text)" : "text-(--text-muted) hover:text-(--text)"
              }`}
            >
              {option.label}
            </button>
          );
        })}
      </div>

      <div className="flex min-h-10 flex-wrap items-baseline gap-(--space-2)">
        {amount === null ? (
          <p data-testid={`billing-price-${plan.id}`} className="text-h3 text-(--text)">
            {QUOTE_ON_REQUEST}
          </p>
        ) : (
          <>
            <p data-testid={`billing-price-${plan.id}`} className="text-h2 text-(--text)">
              {amount}
            </p>
            <p className="text-caption text-(--text-muted)">{plan.price.qualifier}</p>
            {plan.price.discount === null ? null : (
              <span data-testid={`billing-discount-${plan.id}`}>
                <Badge tone="success">{plan.price.discount}</Badge>
              </span>
            )}
          </>
        )}
      </div>

      <a
        href={href}
        target="_blank"
        rel="noreferrer"
        data-testid={`billing-action-${plan.id}`}
        className="text-body inline-flex w-fit items-center gap-(--space-2) rounded-(--radius-control) bg-(--accent) px-(--space-4) py-(--space-2) font-medium text-(--on-accent) hover:bg-(--accent-hover)"
      >
        {planActionLabel(plan)}
        <ArrowUpRight size={14} strokeWidth={1.5} aria-hidden="true" />
      </a>

      <ul className="flex flex-col gap-(--space-1)">
        {plan.highlights.map((highlight) => (
          <li key={highlight} className="text-meta text-(--text-muted)">
            {highlight}
          </li>
        ))}
      </ul>
    </article>
  );
}
