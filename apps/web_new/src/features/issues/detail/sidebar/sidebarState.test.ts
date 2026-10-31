// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { dateBounds, isDueDateOverdue, showsCycleRow, showsEstimateRow, showsModuleRow } from "./sidebarState.js";

describe("row visibility", () => {
  it("shows the module and cycle rows only with their project switches", () => {
    expect(showsModuleRow({ module_view: true })).toBe(true);
    expect(showsModuleRow({ module_view: false })).toBe(false);
    expect(showsCycleRow({ cycle_view: true })).toBe(true);
    expect(showsCycleRow({ cycle_view: false })).toBe(false);
  });

  it("shows the estimate row only when the project runs an estimate system", () => {
    expect(showsEstimateRow({ estimate: "623e4567-e89b-12d3-a456-426614174005" })).toBe(true);
    expect(showsEstimateRow({ estimate: null })).toBe(false);
    expect(showsEstimateRow({ estimate: undefined })).toBe(false);
  });
});

describe("isDueDateOverdue", () => {
  const now = new Date(2026, 9, 18, 12, 0, 0);

  it("flags a due date today or earlier on an open item", () => {
    expect(isDueDateOverdue("2026-10-18", "started", now)).toBe(true);
    expect(isDueDateOverdue("2026-10-01", "unstarted", now)).toBe(true);
    expect(isDueDateOverdue("2026-10-19", "started", now)).toBe(false);
  });

  it("never flags closed states", () => {
    expect(isDueDateOverdue("2026-10-01", "completed", now)).toBe(false);
    expect(isDueDateOverdue("2026-10-01", "cancelled", now)).toBe(false);
  });

  it("never flags without a date or a state group", () => {
    expect(isDueDateOverdue(null, "started", now)).toBe(false);
    expect(isDueDateOverdue(undefined, "started", now)).toBe(false);
    expect(isDueDateOverdue("", "started", now)).toBe(false);
    expect(isDueDateOverdue("2026-10-01", null, now)).toBe(false);
    expect(isDueDateOverdue("2026-10-01", undefined, now)).toBe(false);
  });

  it("compares local calendar days, not the UTC day", () => {
    // A late-evening moment west of UTC already belongs to the next day
    // in UTC; the highlight follows the viewer's own day.
    expect(isDueDateOverdue("2026-10-18", "started", new Date(2026, 9, 18, 23, 30, 0))).toBe(true);
  });
});

describe("dateBounds", () => {
  it("bounds each end by the other", () => {
    expect(dateBounds({ start_date: "2026-10-10", target_date: "2026-10-20" })).toEqual({
      startMax: "2026-10-20",
      dueMin: "2026-10-10",
    });
  });

  it("leaves each bound open when the other end is unset", () => {
    expect(dateBounds({ start_date: null, target_date: null })).toEqual({ startMax: null, dueMin: null });
    expect(dateBounds({ start_date: "2026-10-10", target_date: null })).toEqual({
      startMax: null,
      dueMin: "2026-10-10",
    });
    expect(dateBounds({ start_date: null, target_date: "2026-10-20" })).toEqual({
      startMax: "2026-10-20",
      dueMin: null,
    });
  });

  it("treats absent date fields as unset", () => {
    expect(dateBounds({ start_date: undefined, target_date: undefined })).toEqual({ startMax: null, dueMin: null });
  });
});
