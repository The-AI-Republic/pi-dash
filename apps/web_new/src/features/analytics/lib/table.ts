// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics table foundation (parity/specs/analytics.md §§11, 12,
// 15, 17; ANL-012, ANL-013, ANL-016, ANL-017). Pure: the column model,
// first-column live filter, fixed-noun count labels, and the two
// table column builders. Components render these; nothing here touches
// React, the router, or the network.
import type { AnalyticsCustomChartRow, AnalyticsStatsRow, AnalyticsXAxis, Project } from "@pidash/api-client";

import { customAxisLabel } from "./customAxes.js";
import { selectCustomBarLabel, selectCustomSegmentLabel } from "./customChart.js";

/** Horizontal alignment of a column's cells. */
export type AnalyticsColumnAlign = "left" | "right";

/**
 * One table column. `text` is the visible display string and the live
 * first-column search value; `exportValue` can differ when the file has a
 * documented fallback. `render` is optional for cells that carry a mark.
 */
export interface AnalyticsColumn<Row> {
  key: string;
  label: string;
  align: AnalyticsColumnAlign;
  text: (row: Row) => string;
  exportValue: (row: Row) => unknown;
  render?: (row: Row) => unknown;
}

/** Case-insensitive first-column filter on the visible text. */
export function filterByFirstColumn<Row>(
  rows: readonly Row[],
  query: string,
  columns: readonly AnalyticsColumn<Row>[]
): Row[] {
  const needle = query.trim().toLocaleLowerCase();
  if (needle === "") return [...rows];
  const first = columns[0];
  if (first === undefined) return [];
  return rows.filter((row) => first.text(row).toLocaleLowerCase().includes(needle));
}

/** Display label beside the search box; the noun is fixed, not inflected. */
export function rowCountLabel(count: number, noun: string): string {
  return `${count} ${noun}`;
}

/** A custom-chart table row: the server row plus its display label. */
export interface CustomTableRow {
  source: AnalyticsCustomChartRow;
  label: string;
}

/** Split key order from the response schema. */
export function customSplitKeys(schema: Readonly<Record<string, string>>): string[] {
  return Object.keys(schema);
}

/**
 * Columns under the custom chart. The first column is the current x-axis
 * label, then Count, then one exported split column per schema key. Date
 * and capitalization rules are the chart's own display rules.
 */
export function customTableColumns(
  xAxis: AnalyticsXAxis,
  split: AnalyticsXAxis | null,
  schema: Readonly<Record<string, string>>
): AnalyticsColumn<CustomTableRow>[] {
  const columns: AnalyticsColumn<CustomTableRow>[] = [
    {
      key: "axis",
      label: customAxisLabel(xAxis),
      align: "left",
      text: (row) => row.label,
      exportValue: (row) => row.label,
    },
    {
      key: "count",
      label: "Count",
      align: "right",
      text: (row) => String(row.source.count),
      exportValue: (row) => row.source.count,
    },
  ];
  if (split === null) return columns;
  for (const key of customSplitKeys(schema)) {
    columns.push({
      key,
      label: selectCustomSegmentLabel(schema[key], key, split),
      align: "right",
      text: (row) => String(customSegmentValue(row.source, key)),
      exportValue: (row) => customSegmentValue(row.source, key),
    });
  }
  return columns;
}

/** One split value as the chart reads it: numeric values, else zero. */
export function customSegmentValue(row: AnalyticsCustomChartRow, key: string): number {
  const value: unknown = row[key];
  return typeof value === "number" ? value : 0;
}

/** Build display rows in server order using the chart's label selector. */
export function customTableRows(
  rows: readonly AnalyticsCustomChartRow[],
  xAxis: AnalyticsXAxis,
  currentYear: number
): CustomTableRow[] {
  return rows.map((source) => ({ source, label: selectCustomBarLabel(source.name, xAxis, currentYear) }));
}

/** Project fields the breakdown table reads. */
export type BreakdownProject = Pick<Project, "id" | "name" | "identifier" | "logo_props">;

/** One breakdown row: the server counts plus the live project, if it resolves. */
export interface BreakdownRow {
  stats: AnalyticsStatsRow;
  project: BreakdownProject | undefined;
}

/** Stage count fields in display order with their headers. */
export const BREAKDOWN_STAGES: readonly {
  key: keyof Pick<
    AnalyticsStatsRow,
    | "backlog_work_items"
    | "started_work_items"
    | "un_started_work_items"
    | "completed_work_items"
    | "cancelled_work_items"
  >;
  label: string;
}[] = [
  { key: "backlog_work_items", label: "Backlog" },
  { key: "started_work_items", label: "Started" },
  { key: "un_started_work_items", label: "Unstarted" },
  { key: "completed_work_items", label: "Completed" },
  { key: "cancelled_work_items", label: "Cancelled" },
];

/**
 * Breakdown columns in spec order. The live first cell shows only the
 * resolved project name; the export uses `project__name` as its fallback.
 * The live mark renders for resolved and unresolved ids alike.
 */
export function breakdownTableColumns(): AnalyticsColumn<BreakdownRow>[] {
  return [
    {
      key: "project",
      label: "Project",
      align: "left",
      text: (row) => row.project?.name ?? "",
      // The server name is an export-only fallback. A legacy row can carry
      // no name at all; that column then writes an empty cell rather than a
      // dash, which is reserved for values that cannot be read.
      exportValue: (row) => row.stats.project__name ?? "",
      render: (row) => row.project,
    },
    ...BREAKDOWN_STAGES.map((stage) => ({
      key: stage.key,
      label: stage.label,
      align: "right" as const,
      text: (row: BreakdownRow) => String(row.stats[stage.key]),
      exportValue: (row: BreakdownRow) => row.stats[stage.key],
    })),
  ];
}

/** Join stats rows to the loaded project list without changing server order. */
export function breakdownRows(
  stats: readonly AnalyticsStatsRow[],
  projects: readonly BreakdownProject[]
): BreakdownRow[] {
  const byId = new Map(projects.map((project) => [project.id, project] as const));
  return stats.map((row) => ({ stats: row, project: byId.get(row.project_id) }));
}
