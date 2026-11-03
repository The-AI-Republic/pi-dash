// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom-insights picker model (parity/specs/analytics.md §10,
// ANL-010). Pure: the fixed dimension catalogue in picker order,
// the settled single count metric, the opening values, and the
// mutual-exclusion option selectors. Components render these;
// nothing here touches React, the router, or the network.
import type { AnalyticsXAxis, AnalyticsYAxis } from "@pidash/api-client";

/** One custom-chart dimension: picker label plus request value. */
export interface CustomAxisDef {
  label: string;
  value: AnalyticsXAxis;
}

/** X/split dimensions in the fixed picker order (spec §10). */
export const CUSTOM_AXES: readonly CustomAxisDef[] = [
  { label: "State name", value: "STATES" },
  { label: "State group", value: "STATE_GROUPS" },
  { label: "Priority", value: "PRIORITY" },
  { label: "Label", value: "LABELS" },
  { label: "Assignee", value: "ASSIGNEES" },
  { label: "Estimate point", value: "ESTIMATE_POINTS" },
  { label: "Cycle", value: "CYCLES" },
  { label: "Module", value: "MODULES" },
  { label: "Completed date", value: "COMPLETED_AT" },
  { label: "Due date", value: "TARGET_DATE" },
  { label: "Start date", value: "START_DATE" },
  { label: "Created date", value: "CREATED_AT" },
];

/** One metric option: picker label plus request value. */
export interface CustomMetricDef {
  label: string;
  value: AnalyticsYAxis;
}

/**
 * Metric options. Settled in spec §10/§18: analytics has one
 * supported count metric on the reachable surfaces. The estimate
 * option and the epic/item swap never render, even with points
 * estimation active.
 */
export const CUSTOM_METRICS: readonly CustomMetricDef[] = [{ label: "Work item", value: "WORK_ITEM_COUNT" }];

/** Split option that clears the second split. */
export const CUSTOM_SPLIT_CLEAR_LABEL = "No value";

/** Split trigger label when no second split is chosen (spec §10). */
export const CUSTOM_SPLIT_EMPTY_LABEL = "Add Property";

/** Opening metric (spec §10). */
export const DEFAULT_CUSTOM_METRIC: AnalyticsYAxis = "WORK_ITEM_COUNT";

/** Opening x axis (spec §10). */
export const DEFAULT_CUSTOM_X_AXIS: AnalyticsXAxis = "PRIORITY";

/** Opening split: none (spec §10). */
export const DEFAULT_CUSTOM_SPLIT: null = null;

/** Axes whose server values render as day-grouped date labels. */
const CUSTOM_DATE_AXES: ReadonlySet<AnalyticsXAxis> = new Set([
  "COMPLETED_AT",
  "TARGET_DATE",
  "START_DATE",
  "CREATED_AT",
]);

/** Whether an axis renders its values as day-grouped date labels. */
export function isCustomDateAxis(axis: AnalyticsXAxis): boolean {
  return CUSTOM_DATE_AXES.has(axis);
}

/** Axes whose labels capitalize their first letter for display (spec §10). */
const CUSTOM_CAPITALIZED_AXES: ReadonlySet<AnalyticsXAxis> = new Set(["PRIORITY", "STATE_GROUPS"]);

/** Whether an axis capitalizes its labels' first letter for display. */
export function isCapitalizedCustomAxis(axis: AnalyticsXAxis): boolean {
  return CUSTOM_CAPITALIZED_AXES.has(axis);
}

/** Display label for one axis value, or the empty-split label for none. */
export function customAxisLabel(axis: AnalyticsXAxis | null): string {
  if (axis === null) return CUSTOM_SPLIT_EMPTY_LABEL;
  return CUSTOM_AXES.find((entry) => entry.value === axis)?.label ?? axis;
}

/** Display label for one metric value. */
export function customMetricLabel(metric: AnalyticsYAxis): string {
  return CUSTOM_METRICS.find((entry) => entry.value === metric)?.label ?? metric;
}

/** X options in picker order with the active split removed. */
export function selectCustomXOptions(split: AnalyticsXAxis | null): CustomAxisDef[] {
  return CUSTOM_AXES.filter((entry) => entry.value !== split);
}

/** One split option: the clear row or a dimension. */
export interface CustomSplitDef {
  label: string;
  value: AnalyticsXAxis | null;
}

/**
 * Split options: the clear row first, then every dimension but the
 * current x in picker order.
 */
export function selectCustomSplitOptions(xAxis: AnalyticsXAxis): CustomSplitDef[] {
  return [
    { label: CUSTOM_SPLIT_CLEAR_LABEL, value: null },
    ...CUSTOM_AXES.filter((entry) => entry.value !== xAxis).map((entry) => ({
      label: entry.label,
      value: entry.value as AnalyticsXAxis | null,
    })),
  ];
}
