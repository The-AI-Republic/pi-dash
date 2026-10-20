// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { IssueListItem } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { DAY_WIDTHS } from "./scale.js";
import {
  PARTIAL_BAR_DAYS,
  ROW_OVERSCAN,
  type GanttRow,
  barInView,
  barBox,
  buildRows,
  durationLabel,
  hasBar,
  parseZoom,
  rowWindow,
} from "./rows.js";

function issue(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    name: "Ship it",
    state_id: "22222222-2222-4222-8222-222222222222",
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 7,
    project_id: "33333333-3333-4333-8333-333333333333",
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

const RANGE = { startIso: "2026-10-01", dayCount: 40 };

describe("buildRows", () => {
  it("keys rows with the project identifier and sequence", () => {
    const [row] = buildRows([issue()], "WEB");
    expect(row?.identifier).toBe("WEB-7");
    expect(row?.name).toBe("Ship it");
  });

  it("computes inclusive duration only when both dates exist", () => {
    const [dated, half, undated] = buildRows(
      [
        issue({ id: "a", start_date: "2026-10-01", target_date: "2026-10-05" }),
        issue({ id: "b", start_date: "2026-10-01" }),
        issue({ id: "c" }),
      ],
      "WEB"
    );
    expect(dated?.durationDays).toBe(5);
    expect(half?.durationDays).toBeNull();
    expect(undated?.durationDays).toBeNull();
    expect(durationLabel(dated!)).toBe("5 days");
    expect(durationLabel(half!)).toBeNull();
  });

  it("treats empty date strings as unset", () => {
    const [row] = buildRows([issue({ start_date: "", target_date: null })], "WEB");
    expect(row?.startIso).toBeNull();
    expect(row?.targetIso).toBeNull();
  });

  it("keeps the archived flag", () => {
    const [row] = buildRows([issue({ archived_at: "2026-01-03" })], "WEB");
    expect(row?.archived).toBe(true);
  });

  it("flags temp ids as unconfirmed and leaves server rows confirmed", () => {
    const [temp, real] = buildRows([issue({ id: "temp-abc" }), issue({ id: "real-1" })], "WEB");
    expect(temp?.unconfirmed).toBe(true);
    expect(real?.unconfirmed).toBe(false);
  });
});

describe("barBox", () => {
  const row: GanttRow = {
    id: "a",
    identifier: "WEB-1",
    name: "A",
    startIso: "2026-10-03",
    targetIso: "2026-10-05",
    durationDays: 3,
    stateId: null,
    sortOrder: 1,
    priority: "none",
    sequenceId: 7,
    archived: false,
    unconfirmed: false,
  };

  it("places a dated bar by day column, inclusive", () => {
    expect(barBox(row, RANGE, "week")).toEqual({ left: 2 * DAY_WIDTHS.week, width: 3 * DAY_WIDTHS.week });
  });

  it("keeps a short bar for a half-dated row", () => {
    expect(barBox({ ...row, targetIso: null }, RANGE, "month")).toEqual({
      left: 2 * DAY_WIDTHS.month,
      width: PARTIAL_BAR_DAYS * DAY_WIDTHS.month,
    });
    expect(barBox({ ...row, startIso: null, targetIso: "2026-10-05" }, RANGE, "month")).toEqual({
      left: (4 - (PARTIAL_BAR_DAYS - 1)) * DAY_WIDTHS.month,
      width: PARTIAL_BAR_DAYS * DAY_WIDTHS.month,
    });
  });

  it("draws nothing for an undated row", () => {
    expect(hasBar(row)).toBe(true);
    expect(hasBar({ ...row, startIso: null, targetIso: null })).toBe(false);
    expect(barBox({ ...row, startIso: null, targetIso: null }, RANGE, "week")).toBeNull();
  });

  it("draws an unconfirmed quick-add's seeded bar immediately (ISS-055)", () => {
    expect(hasBar({ ...row, unconfirmed: true })).toBe(true);
    expect(hasBar({ ...row, unconfirmed: true, startIso: null, targetIso: null })).toBe(false);
  });

  it("decides visibility against the visible slice", () => {
    const box = { left: 100, width: 50 };
    expect(barInView(box, 0, 800)).toBe(true);
    expect(barInView(box, 150, 800)).toBe(false);
    expect(barInView(box, 400, 800)).toBe(false);
  });
});

describe("row window", () => {
  it("returns an empty window for an empty list", () => {
    expect(rowWindow(0, 500, 0, 44)).toEqual({ start: 0, end: 0 });
  });

  it("windows around the first visible row with overscan", () => {
    const window = rowWindow(440, 176, 100, 44);
    expect(window.start).toBe(10 - ROW_OVERSCAN);
    expect(window.end).toBe(window.start + 4 + ROW_OVERSCAN * 2);
  });

  it("clamps at the top and the end", () => {
    expect(rowWindow(0, 176, 50, 44).start).toBe(0);
    expect(rowWindow(10_000, 176, 20, 44).end).toBe(20);
    // A zero viewport still renders one row; the end never exceeds the list.
    expect(rowWindow(0, 0, 5, 44)).toEqual({ start: 0, end: 5 });
  });
});

describe("parseZoom", () => {
  it("accepts known zooms and falls back to week", () => {
    expect(parseZoom("quarter")).toBe("quarter");
    expect(parseZoom("day")).toBe("week");
    expect(parseZoom(undefined)).toBe("week");
  });
});
