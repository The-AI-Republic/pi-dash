// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace active cycles (parity/specs/cycles.md §16, CYC-043). The
// paid-feature upsell: the shared upgrade pill beside the header
// (SHELL-101), an on-demand-snapshots heading, six benefit cards, and
// the upgrade action to pricing. It issues no requests — the workspace
// active-cycles endpoint is never read here. The cloud data view is a
// seam: the private overlay carries no active-cycles source at this
// head, so no slice builds a cloud list and the gate proves only this
// upsell. A cloud view, when wanted, arrives behind a new inventory
// row and oracle scenario first.

import { UpgradePill } from "@pidash/kit";
import * as React from "react";

/** Snapshots heading pinned by the oracle. */
export const ACTIVES_SNAPSHOTS_HEADING = "On-demand snapshots of all your cycles";

/** Upgrade action pinned by the oracle. */
export const ACTIVES_UPGRADE_LABEL = "Upgrade";

/** Pricing address the upgrade action opens in a new tab. */
export const ACTIVES_PRICING_HREF = "https://airepublic.com/pricing";

/** One upsell benefit card. */
export interface ActivesBenefit {
  /** Card heading. */
  title: string;
  /** Card body. */
  body: string;
}

/** The six benefit cards in display order. */
export const ACTIVES_BENEFITS: readonly ActivesBenefit[] = [
  {
    title: "Every active cycle in one place",
    body: "See each running cycle across the workspace without opening every project.",
  },
  {
    title: "Progress at a glance",
    body: "Read completion and remaining work per cycle from a single board.",
  },
  {
    title: "Spot stuck cycles early",
    body: "Find the cycles whose burn-down has stalled before the deadline slips.",
  },
  {
    title: "Compare pace across teams",
    body: "Line up throughput cycle over cycle to see which teams need help.",
  },
  {
    title: "Share the picture",
    body: "Send one address that shows leadership the whole workspace state.",
  },
  {
    title: "Keep history handy",
    body: "Revisit finished snapshots to calibrate the next cycle's scope.",
  },
];

export function WorkspaceActivesUpsell(): React.ReactElement {
  return (
    <section
      aria-label="Active cycles"
      data-testid="workspace-actives-upsell"
      className="mx-auto flex w-full max-w-3xl flex-col gap-(--space-6) p-(--space-6)"
    >
      <div className="flex items-center gap-(--space-2)">
        <h1 className="text-title text-(--text)">Active cycles</h1>
        <UpgradePill size="medium" />
      </div>
      <h2 className="text-heading text-(--text-muted)">{ACTIVES_SNAPSHOTS_HEADING}</h2>
      <ul data-testid="workspace-actives-benefits" className="grid gap-(--space-4) md:grid-cols-2">
        {ACTIVES_BENEFITS.map((benefit) => (
          <li
            key={benefit.title}
            className="flex flex-col gap-(--space-2) rounded-(--radius-card) border border-(--border) p-(--space-4)"
          >
            <h3 className="text-emphasis text-(--text)">{benefit.title}</h3>
            <p className="text-body text-(--text-muted)">{benefit.body}</p>
          </li>
        ))}
      </ul>
      <div>
        <a
          href={ACTIVES_PRICING_HREF}
          target="_blank"
          rel="noreferrer"
          className="text-body inline-flex items-center rounded-(--radius-control) bg-(--accent) px-(--space-4) py-(--space-2) font-semibold text-(--on-accent)"
        >
          {ACTIVES_UPGRADE_LABEL}
        </a>
      </div>
    </section>
  );
}
