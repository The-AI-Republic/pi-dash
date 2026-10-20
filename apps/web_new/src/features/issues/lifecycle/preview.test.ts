// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { formatCalendarDay, isOverdue, previewDateLine } from "./preview.js";

const TODAY = "2026-03-10";

describe("formatCalendarDay", () => {
  it("renders month, day and year without timezone math", () => {
    expect(formatCalendarDay("2026-03-10")).toBe("Mar 10, 2026");
    expect(formatCalendarDay("2025-12-31")).toBe("Dec 31, 2025");
  });

  it("returns anything that is not a day string as it came", () => {
    expect(formatCalendarDay("2026-03-10T23:00:00Z")).toBe("2026-03-10T23:00:00Z");
  });

  it("leaves the month blank for a day string outside the calendar", () => {
    expect(formatCalendarDay("2026-13-01")).toBe(" 01, 2026");
  });
});

describe("isOverdue", () => {
  it("flags today and earlier, never a later day or no day", () => {
    expect(isOverdue("2026-03-09", TODAY)).toBe(true);
    expect(isOverdue(TODAY, TODAY)).toBe(true);
    expect(isOverdue("2026-03-11", TODAY)).toBe(false);
    expect(isOverdue(null, TODAY)).toBe(false);
  });
});

describe("previewDateLine", () => {
  it("renders a range when both ends are set", () => {
    expect(previewDateLine("2026-04-01", "2026-04-05", "started", TODAY)).toEqual({
      kind: "range",
      text: "Apr 01, 2026 - Apr 05, 2026",
      overdue: false,
    });
  });

  it("highlights only the due date, not a range that has passed", () => {
    expect(previewDateLine("2026-02-01", "2026-02-05", "started", TODAY)).toEqual({
      kind: "range",
      text: "Feb 01, 2026 - Feb 05, 2026",
      overdue: true,
    });
    expect(previewDateLine("2026-02-01", "2026-02-05", "completed", TODAY)?.overdue).toBe(false);
    expect(previewDateLine("2026-02-01", "2026-02-05", "cancelled", TODAY)?.overdue).toBe(false);
  });

  it("renders a lone start or a lone due date", () => {
    expect(previewDateLine("2026-03-01", null, "started", TODAY)).toEqual({
      kind: "start",
      text: "Mar 01, 2026",
      overdue: false,
    });
    expect(previewDateLine(null, "2026-03-01", "started", TODAY)).toEqual({
      kind: "due",
      text: "Mar 01, 2026",
      overdue: true,
    });
    expect(previewDateLine(null, "2026-04-01", "started", TODAY)?.overdue).toBe(false);
  });

  it("renders nothing when neither end is set", () => {
    expect(previewDateLine(null, null, "started", TODAY)).toBeNull();
  });

  it("keeps an unknown state group unhighlighted-on-finished check honest", () => {
    expect(previewDateLine(null, "2026-03-01", null, TODAY)?.overdue).toBe(true);
    expect(previewDateLine(null, "2026-03-01", "triage", TODAY)?.overdue).toBe(true);
  });
});
