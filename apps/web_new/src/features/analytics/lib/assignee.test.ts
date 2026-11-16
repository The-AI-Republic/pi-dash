// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dialog assignee table model tests (parity/specs/analytics.md §14, ANL-015).
import type { AnalyticsProjectStatsRow } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  ASSIGNEE_COUNT_NOUN,
  assigneeCellText,
  assigneeRows,
  assigneeTableColumns,
  UNASSIGNED_LABEL,
} from "./assignee.js";
import { filterByFirstColumn, rowCountLabel } from "./table.js";

function statsRow(extra: Partial<AnalyticsProjectStatsRow> = {}): AnalyticsProjectStatsRow {
  return {
    display_name: "Ada Lovelace",
    assignee_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    avatar_url: "https://cdn.test/ada.png",
    cancelled_work_items: 0,
    completed_work_items: 1,
    backlog_work_items: 2,
    un_started_work_items: 3,
    started_work_items: 4,
    ...extra,
  };
}

describe("assigneeRows", () => {
  it("keeps server order and identity verbatim", () => {
    const rows = assigneeRows([
      statsRow(),
      statsRow({ display_name: "Bob Stone", assignee_id: null, avatar_url: null }),
      statsRow({ display_name: null, assignee_id: null, avatar_url: null }),
    ]);
    expect(rows.map((row) => row.name)).toEqual(["Ada Lovelace", "Bob Stone", null]);
    expect(rows[0]?.avatarUrl).toBe("https://cdn.test/ada.png");
  });

  it("treats an empty avatar URL as no image", () => {
    expect(assigneeRows([statsRow({ avatar_url: "" })])[0]?.avatarUrl).toBeNull();
  });

  it("returns no rows for an empty read", () => {
    expect(assigneeRows([])).toEqual([]);
  });
});

describe("assigneeCellText", () => {
  it("shows the name, or the marker for the unassigned group", () => {
    const [named, nobody] = assigneeRows([statsRow(), statsRow({ display_name: null })]);
    expect(assigneeCellText(named!)).toBe("Ada Lovelace");
    expect(assigneeCellText(nobody!)).toBe(UNASSIGNED_LABEL);
    expect(UNASSIGNED_LABEL).toBe("Unassigned");
  });
});

describe("assigneeTableColumns", () => {
  it("swaps the first column to assignee and keeps the five stage columns", () => {
    const columns = assigneeTableColumns();
    expect(columns.map((column) => column.label)).toEqual([
      "Assignee",
      "Backlog",
      "Started",
      "Unstarted",
      "Completed",
      "Cancelled",
    ]);
    expect(columns.map((column) => column.align)).toEqual(["left", "right", "right", "right", "right", "right"]);
    expect(columns.map((column) => column.key)).toEqual([
      "assignee",
      "backlog_work_items",
      "started_work_items",
      "un_started_work_items",
      "completed_work_items",
      "cancelled_work_items",
    ]);
  });

  it("reads stage counts from the row and exports the same values", () => {
    const columns = assigneeTableColumns();
    const [row] = assigneeRows([statsRow()]);
    expect(columns.slice(1).map((column) => column.text(row!))).toEqual(["2", "4", "3", "1", "0"]);
    expect(columns.slice(1).map((column) => column.exportValue(row!))).toEqual([2, 4, 3, 1, 0]);
    expect(columns[0]?.exportValue(row!)).toBe("Ada Lovelace");
  });

  it("searches the assignee name and the unassigned marker", () => {
    const columns = assigneeTableColumns();
    const rows = assigneeRows([statsRow(), statsRow({ display_name: null })]);
    expect(filterByFirstColumn(rows, "ada", columns)).toHaveLength(1);
    expect(filterByFirstColumn(rows, "unassigned", columns)).toHaveLength(1);
    expect(filterByFirstColumn(rows, "zzz", columns)).toHaveLength(0);
  });

  it("labels the loaded row count with the fixed assignee noun", () => {
    expect(rowCountLabel(3, ASSIGNEE_COUNT_NOUN)).toBe("3 Assignee");
    expect(ASSIGNEE_COUNT_NOUN).toBe("Assignee");
  });
});
