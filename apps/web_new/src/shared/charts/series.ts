// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared chart series mapping (parity/specs/analytics.md §§6, 9–11,
// NEWFRONT-732). Pure mappers from the wrappers' props-shaped input
// to the flat rows the plotting library consumes, plus the hover
// tooltip shaping. Slices pass display-ready labels; grouping,
// ordering, and coloring decisions stay with the caller.

/** One radar point: a dimension label with its value. */
export interface RadarPoint {
  label: string;
  value: number;
}

/** One radar row for the plotting library. */
export interface RadarDatum {
  dimension: string;
  count: number;
}

/** Radar points in order, renamed onto the plotted keys. */
export function mapRadarData(points: readonly RadarPoint[]): RadarDatum[] {
  return points.map((point) => ({ dimension: point.label, count: point.value }));
}

/** One area point: an axis label with per-series values by series key. */
export interface AreaPoint {
  label: string;
  values: Readonly<Record<string, number>>;
}

/** One area row for the plotting library: the label plus one value per series key. */
export interface AreaDatum {
  label: string;
  [seriesKey: string]: string | number;
}

/**
 * Area points flattened onto `seriesKeys` order. A series missing
 * from a point reads as zero rather than leaving the row ragged.
 */
export function mapAreaData(points: readonly AreaPoint[], seriesKeys: readonly string[]): AreaDatum[] {
  return points.map((point) => {
    const row: AreaDatum = { label: point.label };
    for (const key of seriesKeys) {
      row[key] = point.values[key] ?? 0;
    }
    return row;
  });
}

/** One bar: an axis label with segment values in bottom-to-top order. */
export interface BarInput {
  label: string;
  values: readonly number[];
}

/** One bar row for the plotting library: the label plus one value per segment key. */
export interface BarDatum {
  label: string;
  [segmentKey: string]: string | number;
}

/** Plotted key for the nth segment (bottom-to-top). */
export function barSegmentKey(index: number): string {
  return `segment-${index}`;
}

/**
 * Bars flattened onto `segmentCount` segment keys. A bar shorter
 * than the segment list reads its missing segments as zero.
 */
export function mapBarData(bars: readonly BarInput[], segmentCount: number): BarDatum[] {
  return bars.map((bar) => {
    const row: BarDatum = { label: bar.label };
    for (let index = 0; index < segmentCount; index += 1) {
      row[barSegmentKey(index)] = bar.values[index] ?? 0;
    }
    return row;
  });
}

/** One hovered value, display-shaped. */
export interface ChartTooltipEntry {
  name: string;
  value: string;
  color: string | undefined;
}

/** One legend row, display-shaped. */
export interface ChartLegendEntry {
  label: string;
  color: string | undefined;
}

function isRecord(item: unknown): item is Record<string, unknown> {
  return typeof item === "object" && item !== null;
}

/**
 * One bar's segment values read off a rendered payload row.
 * Non-record payloads read all zero, as do missing segments and
 * non-number values.
 */
export function mapBarPayloadValues(payload: unknown, segmentCount: number): number[] {
  const row: Record<string, unknown> = isRecord(payload) ? payload : {};
  return Array.from({ length: segmentCount }, (_, index) => {
    const raw = row[barSegmentKey(index)];
    return typeof raw === "number" ? raw : 0;
  });
}

/** Tooltip label shaped for display; missing labels read empty. */
export function mapTooltipLabel(label: unknown): string {
  if (typeof label === "string") return label;
  if (label === null || label === undefined) return "";
  return String(label);
}

/**
 * Hovered values shaped for display. Non-list payloads read as no
 * entries, as do entries that carry no object; missing names and
 * values read empty; non-string colors are dropped rather than
 * stringified.
 */
export function mapTooltipEntries(payload: unknown): ChartTooltipEntry[] {
  if (!Array.isArray(payload)) return [];
  return payload.filter(isRecord).map((item) => ({
    name: typeof item["name"] === "string" ? item["name"] : String(item["name"] ?? ""),
    value: typeof item["value"] === "string" ? item["value"] : String(item["value"] ?? ""),
    color: typeof item["color"] === "string" ? item["color"] : undefined,
  }));
}

/**
 * Legend rows shaped for display. Non-list payloads read as no
 * rows; missing labels read empty; non-string colors are dropped
 * rather than stringified.
 */
export function mapLegendEntries(payload: unknown): ChartLegendEntry[] {
  if (!Array.isArray(payload)) return [];
  return payload.filter(isRecord).map((item) => ({
    label: typeof item["value"] === "string" ? item["value"] : String(item["value"] ?? ""),
    color: typeof item["color"] === "string" ? item["color"] : undefined,
  }));
}
