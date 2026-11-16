// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics table foundation tests (parity/specs/analytics.md §§11, 12,
// 15, 17; ANL-012, ANL-013, ANL-016, ANL-017).
import type { AnalyticsCustomChartRow, AnalyticsStatsRow, Project } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  breakdownRows,
  breakdownTableColumns,
  customSegmentValue,
  customSplitKeys,
  customTableColumns,
  customTableRows,
  filterByFirstColumn,
  rowCountLabel,
  type AnalyticsColumn,
  type BreakdownRow,
} from "./table.js";

interface SimpleRow {
  name: string;
  count: number;
}

const SIMPLE_COLUMNS: AnalyticsColumn<SimpleRow>[] = [
  { key: "name", label: "Name", align: "left", text: (row) => row.name, exportValue: (row) => row.name },
  { key: "count", label: "Count", align: "right", text: (row) => String(row.count), exportValue: (row) => row.count },
];

function customRow(name: string, count: number, extra: Record<string, unknown> = {}): AnalyticsCustomChartRow {
  return { key: name, name, count, ...extra };
}

function statsRow(projectId: string, name: string): AnalyticsStatsRow {
  return {
    project_id: projectId,
    project__name: name,
    cancelled_work_items: 5,
    completed_work_items: 4,
    backlog_work_items: 1,
    un_started_work_items: 2,
    started_work_items: 3,
  };
}

function project(id: string, name: string): Pick<Project, "id" | "name" | "identifier" | "logo_props"> {
  return { id, name, identifier: name.slice(0, 2).toUpperCase(), logo_props: {} };
}

describe("first-column filtering", () => {
  it("filters case-insensitively on the visible first column", () => {
    const rows = [
      { name: "Urgent", count: 1 },
      { name: "High", count: 2 },
      { name: "urgent follow-up", count: 3 },
    ];
    expect(filterByFirstColumn(rows, "URG", SIMPLE_COLUMNS)).toEqual([
      { name: "Urgent", count: 1 },
      { name: "urgent follow-up", count: 3 },
    ]);
    expect(filterByFirstColumn(rows, "2", SIMPLE_COLUMNS)).toEqual([]);
  });

  it("returns a copy of every row for a blank query so callers can export safely", () => {
    const rows = [{ name: "Urgent", count: 1 }];
    const filtered = filterByFirstColumn(rows, "   ", SIMPLE_COLUMNS);
    expect(filtered).toEqual(rows);
    expect(filtered).not.toBe(rows);
  });

  it("returns no rows when the model has no first column", () => {
    expect(filterByFirstColumn([{ name: "Urgent", count: 1 }], "urgent", [])).toEqual([]);
  });
});

describe("rowCountLabel", () => {
  it("uses the fixed noun without inflection", () => {
    expect(rowCountLabel(0, "Projects")).toBe("0 Projects");
    expect(rowCountLabel(1, "Projects")).toBe("1 Projects");
    expect(rowCountLabel(3, "Priority")).toBe("3 Priority");
  });
});

describe("custom chart table", () => {
  it("builds ungrouped columns from the x-axis label and Count", () => {
    const columns = customTableColumns("PRIORITY", null, {});
    expect(columns.map((column) => column.label)).toEqual(["Priority", "Count"]);
    expect(columns.map((column) => column.align)).toEqual(["left", "right"]);
  });

  it("adds one right-aligned column per schema key in order for a split", () => {
    const columns = customTableColumns("PRIORITY", "PRIORITY", { u: "Urgent", h: "High" });
    expect(columns.map((column) => column.label)).toEqual(["Priority", "Count", "Urgent", "High"]);
    expect(columns.map((column) => column.align)).toEqual(["left", "right", "right", "right"]);
    expect(customSplitKeys({ h: "High", u: "Urgent" })).toEqual(["h", "u"]);
  });

  it("uses the chart's date and capitalization display labels in the table", () => {
    const rows = customTableRows(
      [customRow("2024-03-15", 1), customRow("urgent", 2), customRow("", 0)],
      "PRIORITY",
      2026
    );
    expect(rows.map((row) => row.label)).toEqual(["2024-03-15", "Urgent", ""]);
    const due = customTableRows([customRow("2024-03-15", 1), customRow("2026-10-23", 2)], "TARGET_DATE", 2026);
    expect(due.map((row) => row.label)).toEqual(["Mar 15, 2024", "Oct 23"]);
  });

  it("writes numeric segment values and reads missing or non-numeric values as zero", () => {
    const row = customRow("urgent", 3, { a: 1, b: "two" });
    expect(customSegmentValue(row, "a")).toBe(1);
    expect(customSegmentValue(row, "b")).toBe(0);
    expect(customSegmentValue(row, "missing")).toBe(0);
  });

  it("exports display text and counts from the same row model", () => {
    const columns = customTableColumns("PRIORITY", "LABELS", { l1: "Alpha", l2: "Beta" });
    const rows = customTableRows([customRow("urgent", 3, { l1: 1, l2: 2 })], "PRIORITY", 2026);
    expect(columns.map((column) => column.text(rows[0]!))).toEqual(["Urgent", "3", "1", "2"]);
    expect(columns.map((column) => column.exportValue(rows[0]!))).toEqual(["Urgent", 3, 1, 2]);
  });
});

describe("state breakdown table", () => {
  it("builds the six columns in spec order", () => {
    expect(breakdownTableColumns().map((column) => column.label)).toEqual([
      "Project",
      "Backlog",
      "Started",
      "Unstarted",
      "Completed",
      "Cancelled",
    ]);
  });

  it("joins rows in server order and resolves the live project", () => {
    const rows = breakdownRows(
      [statsRow("b", "Server B"), statsRow("a", "Server A")],
      [project("a", "Alpha"), project("b", "Beta")]
    );
    expect(rows.map((row) => row.project?.name)).toEqual(["Beta", "Alpha"]);
  });

  it("shows a blank live name for an unresolved project but exports the server name", () => {
    const rows = breakdownRows([statsRow("gone", "Fallback")], []);
    const columns = breakdownTableColumns();
    expect(columns[0]?.text(rows[0] as BreakdownRow)).toBe("");
    expect(columns[0]?.exportValue(rows[0] as BreakdownRow)).toBe("Fallback");
    expect(columns[0]?.render?.(rows[0] as BreakdownRow)).toBeUndefined();
  });

  it("exports an absent server project name as an empty cell, never a dash", () => {
    const row = breakdownRows([statsRow("gone", "Fallback")], [])[0] as BreakdownRow;
    row.stats.project__name = null as unknown as string;
    expect(breakdownTableColumns()[0]?.exportValue(row)).toBe("");
  });

  it("reads every stage count from its column, including zero", () => {
    const row = breakdownRows([statsRow("a", "Alpha")], [project("a", "Alpha")])[0] as BreakdownRow;
    const columns = breakdownTableColumns();
    expect(columns.slice(1).map((column) => column.text(row))).toEqual(["1", "3", "2", "4", "5"]);
    expect(columns.slice(1).map((column) => column.exportValue(row))).toEqual([1, 3, 2, 4, 5]);
  });
});
