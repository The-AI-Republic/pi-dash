// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-cycle analytics selectors: the states breakdown from the
// archived counts, the assignee/label rows from the loose
// distribution records, and the burndown chart coercion.

import type { ArchivedCycleDetail } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  ARCHIVED_CYCLE_NO_ASSIGNEE_TITLE,
  ARCHIVED_CYCLE_NO_LABEL_TITLE,
  selectArchivedCycleAssigneeRows,
  selectArchivedCycleChart,
  selectArchivedCycleLabelRows,
  selectArchivedCycleStateRows,
} from "./archivedCycleAnalytics.js";

const CYCLE = "323e4567-e89b-12d3-a456-426614174002";

function archivedCycle(overrides: Partial<ArchivedCycleDetail> = {}): ArchivedCycleDetail {
  return {
    id: CYCLE,
    workspace_id: "123e4567-e89b-12d3-a456-426614174000",
    project_id: "223e4567-e89b-12d3-a456-426614174001",
    name: "Archived cycle",
    description: "",
    start_date: "2026-01-01T00:00:00Z",
    end_date: "2026-02-01T00:00:00Z",
    owned_by_id: "123e4567-e89b-12d3-a456-426614174010",
    status: "COMPLETED",
    archived_at: "2026-03-01T00:00:00Z",
    is_favorite: false,
    total_issues: 4,
    completed_issues: 3,
    started_issues: 1,
    unstarted_issues: 0,
    cancelled_issues: 0,
    backlog_issues: 0,
    assignee_ids: [],
    sub_issues: 0,
    completed_estimate_points: 0,
    total_estimate_points: 0,
    estimate_distribution: {},
    distribution: { assignees: [], labels: [], completion_chart: null },
    ...overrides,
  };
}

describe("selectArchivedCycleStateRows", () => {
  it("orders the four open groups with shares over the cancelled-included total", () => {
    const rows = selectArchivedCycleStateRows(archivedCycle());

    expect(rows.map((row) => [row.id, row.title, row.count, row.total, row.percent])).toEqual([
      ["backlog", "Backlog", 0, 4, 0],
      ["unstarted", "Unstarted", 0, 4, 0],
      ["started", "Started", 1, 4, 25],
      ["completed", "Completed", 3, 4, 75],
    ]);
  });

  it("appends the cancelled group only when it holds items", () => {
    const rows = selectArchivedCycleStateRows(
      archivedCycle({ total_issues: 5, completed_issues: 3, started_issues: 1, cancelled_issues: 1 })
    );

    expect(rows.map((row) => row.id)).toEqual(["backlog", "unstarted", "started", "completed", "cancelled"]);
    expect(rows.at(-1)).toMatchObject({ title: "Cancelled", count: 1, total: 5, percent: 20 });
  });

  it("shares nothing on a zero total", () => {
    const rows = selectArchivedCycleStateRows(
      archivedCycle({
        total_issues: 0,
        completed_issues: 0,
        started_issues: 0,
        distribution: { assignees: [], labels: [], completion_chart: null },
      })
    );

    expect(rows.every((row) => row.percent === 0)).toBe(true);
  });
});

describe("selectArchivedCycleAssigneeRows", () => {
  it("reads ids, names and counts in server order", () => {
    const rows = selectArchivedCycleAssigneeRows(
      archivedCycle({
        distribution: {
          assignees: [
            { display_name: "Ada Lovelace", assignee_id: "aaaaaaaa-0000-4000-8000-000000000001", total_issues: 3 },
            { display_name: "Grace Hopper", assignee_id: "aaaaaaaa-0000-4000-8000-000000000003", total_issues: 1 },
          ],
          labels: [],
          completion_chart: null,
        },
      })
    );

    expect(rows).toEqual([
      { id: "aaaaaaaa-0000-4000-8000-000000000001", title: "Ada Lovelace", count: 3, total: 4, percent: 75 },
      { id: "aaaaaaaa-0000-4000-8000-000000000003", title: "Grace Hopper", count: 1, total: 4, percent: 25 },
    ]);
  });

  it("falls back on the unassigned title and skips shapeless entries", () => {
    const rows = selectArchivedCycleAssigneeRows(
      archivedCycle({
        distribution: {
          assignees: [{ display_name: null, assignee_id: null, total_issues: 2 }, null, "oops"] as unknown as Array<
            Record<string, unknown>
          >,
          labels: [],
          completion_chart: null,
        },
      })
    );

    expect(rows).toEqual([{ id: null, title: ARCHIVED_CYCLE_NO_ASSIGNEE_TITLE, count: 2, total: 4, percent: 50 }]);
  });
});

describe("selectArchivedCycleLabelRows", () => {
  it("reads ids, names and counts with the unlabeled fallback", () => {
    const rows = selectArchivedCycleLabelRows(
      archivedCycle({
        distribution: {
          assignees: [],
          labels: [
            { label_name: "Bug", label_id: "bbbbbbbb-0000-4000-8000-000000000002", total_issues: 2 },
            { label_name: null, label_id: null, total_issues: 2 },
          ],
          completion_chart: null,
        },
      })
    );

    expect(rows).toEqual([
      { id: "bbbbbbbb-0000-4000-8000-000000000002", title: "Bug", count: 2, total: 4, percent: 50 },
      { id: null, title: ARCHIVED_CYCLE_NO_LABEL_TITLE, count: 2, total: 4, percent: 50 },
    ]);
  });
});

describe("selectArchivedCycleChart", () => {
  it("passes the date-to-remaining map through", () => {
    const chart = selectArchivedCycleChart(
      archivedCycle({
        distribution: { assignees: [], labels: [], completion_chart: { "2026-01-05": 3, "2026-01-06": null } },
      })
    );

    expect(chart).toEqual({ "2026-01-05": 3, "2026-01-06": null });
  });

  it("reads a missing chart as null", () => {
    expect(selectArchivedCycleChart(archivedCycle())).toBeNull();
  });

  it("skips entries that are neither numbers nor nulls", () => {
    const chart = selectArchivedCycleChart(
      archivedCycle({
        distribution: { assignees: [], labels: [], completion_chart: { "2026-01-05": 3, "2026-01-06": "x" } },
      })
    );

    expect(chart).toEqual({ "2026-01-05": 3 });
  });
});
