// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Hero burn-down card (parity/specs/cycles.md §15, CYC-041): the
// pending line, the remaining-work chart per measure, and the measure
// switch the breakdown shares. Undated and future-start cycles show
// the chart placeholder with the line and switch intact; a cycle
// without work shows the card's empty view instead.

import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useResolvedScheme } from "../../../core/theme/index.js";
import { AreaChart, chartSlotColor, resolveChartScheme } from "../../../shared/charts/index.js";
import { heroAnalyticsQueryOptions, heroProgressQueryOptions } from "../api/heroQueries.js";
import {
  heroBurndownPoints,
  heroChartAvailable,
  heroMeasureAvailable,
  heroPendingCount,
  type HeroMeasure,
} from "../lib/cycleHero.js";

/** Card heading. */
export const HERO_BURNDOWN_TITLE = "Work item burndown";
/** Empty view when the cycle holds no work. */
export const HERO_BURNDOWN_EMPTY = "No burn-down data yet";
/** Chart placeholder for undated and future-start cycles. */
export const HERO_BURNDOWN_NO_CHART = "The chart appears for a dated cycle that has started.";

export interface HeroBurndownCardProps {
  workspaceSlug: string;
  projectId: string;
  cycleId: string;
  startDate: string | null | undefined;
  endDate: string | null | undefined;
  measure: HeroMeasure;
  /** Estimates ride the switch only where the project enables them. */
  estimatesEnabled: boolean;
  /** Active estimate family; categories never offers a numeric measure. */
  estimateType: string | null | undefined;
  onMeasureChange: (measure: HeroMeasure) => void;
}

export function HeroBurndownCard({
  workspaceSlug,
  projectId,
  cycleId,
  startDate,
  endDate,
  measure,
  estimatesEnabled,
  estimateType,
  onMeasureChange,
}: HeroBurndownCardProps): React.ReactElement {
  const client = getAppClient();
  const progress = useQuery(heroProgressQueryOptions(client, workspaceSlug, projectId, cycleId));
  const measureAvailable = heroMeasureAvailable({
    estimatesEnabled,
    estimateType,
    totalEstimatePoints: progress.data?.total_estimate_points,
  });
  // Estimates-off projects read back as counts even when point data
  // keeps the switch visible.
  const activeMeasure: HeroMeasure = estimatesEnabled && measureAvailable ? measure : "issues";
  const analytics = useQuery(heroAnalyticsQueryOptions(client, workspaceSlug, projectId, cycleId, activeMeasure));
  const { scheme, customDark } = useResolvedScheme();
  const data = progress.data;
  const chart = analytics.data?.completion_chart;

  let body: React.ReactNode;
  if (progress.isPending || analytics.isPending) {
    body = (
      <>
        <h3 className="text-emphasis text-(--text)">{HERO_BURNDOWN_TITLE}</h3>
        <p data-testid="hero-burndown-loading" className="text-meta text-(--text-muted)">
          Loading burn-down…
        </p>
      </>
    );
  } else if (
    progress.isError ||
    analytics.isError ||
    data === undefined ||
    chart === undefined ||
    data.total_issues === 0
  ) {
    body = (
      <div className="text-center">
        <h3 className="text-body text-(--text-muted)">{HERO_BURNDOWN_EMPTY}</h3>
      </div>
    );
  } else {
    const points = heroBurndownPoints(chart, activeMeasure).map((point) => ({
      label: point.day,
      values: { remaining: point.remaining },
    }));
    body = (
      <>
        <h3 className="text-emphasis text-(--text)">{HERO_BURNDOWN_TITLE}</h3>
        <div className="flex items-center justify-between gap-(--space-2)">
          <span data-testid="hero-burndown-pending" className="text-body text-(--text)">
            {heroPendingCount(data)}
          </span>
          {measureAvailable ? (
            <div role="group" aria-label="Measure" className="flex items-center gap-(--space-1)">
              <button
                type="button"
                aria-pressed={activeMeasure === "issues"}
                onClick={() => onMeasureChange("issues")}
                className="text-meta cursor-pointer rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) data-[active=true]:bg-(--subtle) data-[active=true]:text-(--text)"
                data-active={activeMeasure === "issues"}
              >
                Items
              </button>
              <button
                type="button"
                aria-pressed={activeMeasure === "points"}
                onClick={() => onMeasureChange("points")}
                className="text-meta cursor-pointer rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) data-[active=true]:bg-(--subtle) data-[active=true]:text-(--text)"
                data-active={activeMeasure === "points"}
              >
                Estimates
              </button>
            </div>
          ) : null}
        </div>
        {heroChartAvailable(startDate, endDate) ? (
          <AreaChart
            points={points}
            series={[
              {
                key: "remaining",
                name: "Remaining work items",
                color: chartSlotColor(0, resolveChartScheme(scheme, customDark)),
              },
            ]}
            legend={false}
            height={220}
          />
        ) : (
          <p data-testid="hero-burndown-placeholder" className="text-meta text-(--text-muted)">
            {HERO_BURNDOWN_NO_CHART}
          </p>
        )}
      </>
    );
  }
  return (
    <section
      data-testid="hero-burndown-card"
      aria-label={HERO_BURNDOWN_TITLE}
      className="flex flex-col gap-(--space-3) rounded-(--radius-card) border border-(--border) bg-(--surface) p-(--space-4)"
    >
      {body}
    </section>
  );
}
