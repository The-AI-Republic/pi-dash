// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom bar-chart shaping (parity/specs/analytics.md §10,
// ANL-011). Pure: server rows become labeled bars with segment
// values, segment names and colors, and per-bar fills. Priority
// bars use the fixed spec hues, state bars the workspace state
// colors with a palette fallback, date bars day-grouped
// year-aware labels, and stacked segments the kit categorical
// slots in schema order. Components pass the result to the
// shared bar wrapper; nothing here touches React or the network.
import type { ChartScheme } from "@pidash/kit";
import type { AnalyticsCustomChartRow, AnalyticsXAxis, StateList } from "@pidash/api-client";

import { chartSlotColor } from "../../../shared/charts/colors.js";

import { isCapitalizedCustomAxis, isCustomDateAxis } from "./customAxes.js";

/** Single-series tooltip name (spec §10). */
export const CUSTOM_SINGLE_SERIES_NAME = "Count";

/**
 * Fixed single-series hues by lowercase priority key (spec §10).
 * Matched case-insensitively against the server keys.
 */
const PRIORITY_BAR_HUES: Readonly<Record<string, string>> = {
  urgent: "#ef4444",
  high: "#f97316",
  medium: "#eab308",
  low: "#22c55e",
  none: "#ced4da",
};

/** Display label when a bucket carries no value. */
const EMPTY_BUCKET_LABEL = "None";

/** Short month names for date labels. */
const CUSTOM_MONTHS: readonly string[] = [
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

/** A calendar date bucket as the server sends it. */
const CUSTOM_DATE_PATTERN = /^(\d{4})-(\d{2})-(\d{2})$/;

/** Capitalize the first letter, leaving the rest untouched. */
export function capitalizeCustomLabel(name: string): string {
  if (name === "") return name;
  return name.slice(0, 1).toUpperCase() + name.slice(1);
}

/**
 * Display label for one date-axis value. Missing values read
 * `None`; current-year dates read `MMM D`, other years
 * `MMM D, YYYY`; anything else stays verbatim (spec §10).
 */
export function formatCustomDateLabel(name: string, currentYear: number): string {
  const text = name.trim();
  if (text === "" || text.toLowerCase() === "none") return EMPTY_BUCKET_LABEL;
  const match = CUSTOM_DATE_PATTERN.exec(text);
  if (match === null) return name;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  if (month < 1 || month > 12 || day < 1 || day > 31) return name;
  const label = `${CUSTOM_MONTHS[month - 1]} ${day}`;
  return year === currentYear ? label : `${label}, ${year}`;
}

/** Display label for one x value on the given axis. */
export function selectCustomBarLabel(name: string, xAxis: AnalyticsXAxis, currentYear: number): string {
  if (isCustomDateAxis(xAxis)) return formatCustomDateLabel(name, currentYear);
  if (isCapitalizedCustomAxis(xAxis)) return capitalizeCustomLabel(name);
  return name;
}

/** Display label for one stacked segment on the given split. */
export function selectCustomSegmentLabel(
  schemaLabel: string | undefined,
  schemaKey: string,
  split: AnalyticsXAxis
): string {
  const label = schemaLabel ?? schemaKey;
  return isCapitalizedCustomAxis(split) ? capitalizeCustomLabel(label) : label;
}

/** State colors by id and by name; the first row wins each key. */
export interface StateColorIndex {
  byId: ReadonlyMap<string, string>;
  byName: ReadonlyMap<string, string>;
}

/** Index workspace states for bar coloring. */
export function indexStateColors(states: StateList): StateColorIndex {
  const byId = new Map<string, string>();
  const byName = new Map<string, string>();
  for (const state of states) {
    if (!byId.has(state.id)) byId.set(state.id, state.color);
    if (!byName.has(state.name)) byName.set(state.name, state.color);
  }
  return { byId, byName };
}

/**
 * Single-series fill for one priority bar: the fixed hue for its
 * key, or the bar's palette slot for an unknown key.
 */
export function selectPriorityBarColor(key: string | number, slot: number, scheme: ChartScheme): string {
  return PRIORITY_BAR_HUES[String(key).toLowerCase()] ?? chartSlotColor(slot, scheme);
}

/**
 * Single-series fill for one state bar: the workspace state's
 * configured color by id, then by name, or the bar's palette slot
 * when the state no longer resolves (spec §10).
 */
export function selectStateBarColor(
  key: string | number,
  name: string,
  slot: number,
  scheme: ChartScheme,
  colors: StateColorIndex
): string {
  const text = String(key);
  return colors.byId.get(text) ?? colors.byName.get(name) ?? chartSlotColor(slot, scheme);
}

/** One stacked segment value off a grouped row; missing values read zero. */
function segmentValue(row: AnalyticsCustomChartRow, schemaKey: string): number {
  const value: unknown = row[schemaKey];
  return typeof value === "number" ? value : 0;
}

/** One shaped bar: its display label with values in segment order. */
export interface CustomChartBar {
  label: string;
  values: number[];
}

/** One shaped segment: its tooltip name with its color. */
export interface CustomChartSegment {
  name: string;
  color: string;
}

/** Bars, segments, and per-bar fills for the shared bar wrapper. */
export interface CustomChartModel {
  bars: CustomChartBar[];
  segments: CustomChartSegment[];
  /** Per-bar fills in plotted order; set in single-series mode only. */
  fills: string[] | undefined;
}

/** Inputs the shaper reads; rows and schema keep server order. */
export interface CustomChartInput {
  rows: readonly AnalyticsCustomChartRow[];
  schema: Readonly<Record<string, string>>;
  xAxis: AnalyticsXAxis;
  split: AnalyticsXAxis | null;
  /** Workspace states; read only for single-series state axes. */
  states: StateList;
  scheme: ChartScheme;
  currentYear: number;
}

/** Per-bar fill for one single-series bar on the given axis. */
function singleBarColor(
  row: AnalyticsCustomChartRow,
  slot: number,
  input: CustomChartInput,
  stateColors: StateColorIndex
): string {
  if (input.xAxis === "PRIORITY") return selectPriorityBarColor(row.key, slot, input.scheme);
  if (input.xAxis === "STATES") return selectStateBarColor(row.key, row.name, slot, input.scheme, stateColors);
  return chartSlotColor(slot, input.scheme);
}

/**
 * Shape one custom-graph answer for the shared bar wrapper. Rows
 * and schema keys keep server order. Without a split there is one
 * `Count` series with per-bar fills; with a split each schema key
 * stacks in order on its palette slot (spec §10).
 */
export function buildCustomChartModel(input: CustomChartInput): CustomChartModel {
  const split = input.split;
  if (split === null) {
    const stateColors = indexStateColors(input.states);
    return {
      bars: input.rows.map((row) => ({
        label: selectCustomBarLabel(row.name, input.xAxis, input.currentYear),
        values: [row.count],
      })),
      segments: [{ name: CUSTOM_SINGLE_SERIES_NAME, color: chartSlotColor(0, input.scheme) }],
      fills: input.rows.map((row, index) => singleBarColor(row, index, input, stateColors)),
    };
  }
  const schemaKeys = Object.keys(input.schema);
  return {
    bars: input.rows.map((row) => ({
      label: selectCustomBarLabel(row.name, input.xAxis, input.currentYear),
      values: schemaKeys.map((key) => segmentValue(row, key)),
    })),
    segments: schemaKeys.map((key, index) => ({
      name: selectCustomSegmentLabel(input.schema[key], key, split),
      color: chartSlotColor(index, input.scheme),
    })),
    fills: undefined,
  };
}
