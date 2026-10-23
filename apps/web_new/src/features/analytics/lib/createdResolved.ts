// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Created-vs-resolved shaping (parity/specs/analytics.md §9,
// ANL-009). Pure: month-bucket labels in calendar form, the two
// series' keys and legend names, and the series colors per chart
// scheme. Components pass the result to the shared area wrapper;
// nothing here touches React or the network.
import type { ChartScheme } from "@pidash/kit";
import type { AnalyticsWorkItemsChartPoint } from "@pidash/api-client";

/** Series key for the resolved (completed) line. */
export const RESOLVED_SERIES_KEY = "resolved";

/** Series key for the created line. */
export const CREATED_SERIES_KEY = "created";

/** Legend name for the resolved line, plotted first (spec §9). */
export const RESOLVED_SERIES_NAME = "Resolved";

/** Legend name for the created line (spec §9). */
export const CREATED_SERIES_NAME = "Created";

/** Short month names for bucket labels. */
const SERIES_MONTHS: readonly string[] = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
];

/** A month bucket key as the server sends it. */
const SERIES_BUCKET_PATTERN = /^(\d{4})-(\d{2})-(\d{2})$/;

/**
 * Calendar label for one bucket key (`2026-10-01` reads
 * `Oct 01, 2026`). Anything outside the bucket shape stays
 * verbatim rather than becoming a wrong date.
 */
export function formatSeriesMonth(bucket: string): string {
  const match = SERIES_BUCKET_PATTERN.exec(bucket.trim());
  if (match === null) return bucket;
  // A successful match always captures all three groups.
  const year = match[1]!;
  const month = Number(match[2]);
  const day = match[3]!;
  if (month < 1 || month > 12 || !/^(0[1-9]|[12][0-9]|3[01])$/.test(day)) return bucket;
  return `${SERIES_MONTHS[month - 1]} ${day}, ${year}`;
}

/** Concrete colors for the two series. */
export interface CreatedResolvedColors {
  resolved: string;
  created: string;
}

/**
 * Series colors per chart scheme. Resolved mirrors the success
 * token; created is a feature-chosen blue — the oracle pins the
 * series and legend, not the fills.
 */
export function selectCreatedResolvedColors(scheme: ChartScheme): CreatedResolvedColors {
  if (scheme === "dark") return { resolved: "#3db86c", created: "#60a5fa" };
  return { resolved: "#2f9e5b", created: "#2563eb" };
}

/** One shaped point: its calendar label with per-series values. */
export interface CreatedResolvedPoint {
  label: string;
  values: Record<string, number>;
}

/** Shape server buckets into labeled area points in server order. */
export function selectCreatedResolvedPoints(rows: readonly AnalyticsWorkItemsChartPoint[]): CreatedResolvedPoint[] {
  return rows.map((row) => ({
    label: formatSeriesMonth(row.name),
    values: { [RESOLVED_SERIES_KEY]: row.completed_issues, [CREATED_SERIES_KEY]: row.created_issues },
  }));
}
