// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline gesture math (parity/specs/issues.md §6, ISS-050–055).
import { describe, expect, it } from "vitest";

import {
  DRAG_THRESHOLD_PX,
  RANK_STEP,
  addBlockDate,
  baseWindow,
  dayIndexAt,
  deriveReorder,
  handlePreviewDate,
  movedDates,
  plantedDates,
  resizedDates,
  snappedDayDelta,
  windowDays,
  type ReorderEntry,
} from "./drag.js";

const ENTRIES: ReorderEntry[] = [
  { id: "a", sortOrder: 1000 },
  { id: "b", sortOrder: 2000 },
  { id: "c", sortOrder: 3000 },
];

describe("deriveReorder (ISS-050)", () => {
  it("inserts before the target and takes the midpoint of its neighbours", () => {
    expect(deriveReorder(ENTRIES, "c", "b", "before")).toEqual({
      order: ["a", "c", "b"],
      sortOrder: 1500,
    });
  });

  it("inserts after the target", () => {
    expect(deriveReorder(ENTRIES, "a", "b", "after")).toEqual({
      order: ["b", "a", "c"],
      sortOrder: 2500,
    });
  });

  it("steps below the first row when dropped at the top", () => {
    expect(deriveReorder(ENTRIES, "c", "a", "before")).toEqual({
      order: ["c", "a", "b"],
      sortOrder: 1000 - RANK_STEP,
    });
  });

  it("steps above the last row when dropped at the bottom", () => {
    expect(deriveReorder(ENTRIES, "a", "c", "after")).toEqual({
      order: ["b", "c", "a"],
      sortOrder: 3000 + RANK_STEP,
    });
  });

  it("uses a single step when the list holds one row", () => {
    expect(deriveReorder([{ id: "only", sortOrder: 5 }], "only", "only")).toBeNull();
  });

  it("ignores unknown ids, self drops and no-op positions", () => {
    expect(deriveReorder(ENTRIES, "z", "a")).toBeNull();
    expect(deriveReorder(ENTRIES, "a", "z")).toBeNull();
    expect(deriveReorder(ENTRIES, "a", "a")).toBeNull();
    expect(deriveReorder(ENTRIES, "a", "b", "before")).toBeNull();
  });
});

describe("snapping and columns", () => {
  it("rounds travel to the nearest day", () => {
    expect(snappedDayDelta(39, 40)).toBe(1);
    expect(snappedDayDelta(19, 40)).toBe(0);
    expect(snappedDayDelta(-61, 40)).toBe(-2);
  });

  it("rejects a zero or invalid day width", () => {
    expect(snappedDayDelta(100, 0)).toBe(0);
    expect(snappedDayDelta(Number.NaN, 40)).toBe(0);
  });

  it("resolves the hovered column and never goes negative", () => {
    expect(dayIndexAt(0, 40)).toBe(0);
    expect(dayIndexAt(81, 40)).toBe(2);
    expect(dayIndexAt(-12, 40)).toBe(0);
    expect(dayIndexAt(10, 0)).toBe(0);
  });

  it("counts a drag threshold for click suppression", () => {
    expect(DRAG_THRESHOLD_PX).toBeGreaterThan(0);
  });
});

describe("plantedDates (ISS-054)", () => {
  it("plants one day in week and month zoom", () => {
    expect(plantedDates("2026-10-01", 2, "week")).toEqual({ startDate: "2026-10-03", targetDate: "2026-10-04" });
    expect(plantedDates("2026-10-01", 2, "month")).toEqual({ startDate: "2026-10-03", targetDate: "2026-10-04" });
  });

  it("plants a week in quarter zoom", () => {
    expect(plantedDates("2026-10-01", 0, "quarter")).toEqual({ startDate: "2026-10-01", targetDate: "2026-10-08" });
  });

  it("clamps a negative column", () => {
    expect(plantedDates("2026-10-01", -3, "week").startDate).toBe("2026-10-01");
  });

  it("labels the add control with the hovered day", () => {
    expect(addBlockDate({ startIso: "2026-10-01" }, 4)).toBe("2026-10-05");
  });
});

describe("baseWindow", () => {
  it("keeps a fully dated window", () => {
    expect(baseWindow({ startDate: "2026-10-01", targetDate: "2026-10-05" })).toEqual({
      startDate: "2026-10-01",
      targetDate: "2026-10-05",
    });
  });

  it("fills the missing end from the known one", () => {
    expect(baseWindow({ startDate: "2026-10-01", targetDate: null })).toEqual({
      startDate: "2026-10-01",
      targetDate: "2026-10-02",
    });
    expect(baseWindow({ startDate: null, targetDate: "2026-10-05" })).toEqual({
      startDate: "2026-10-04",
      targetDate: "2026-10-05",
    });
  });

  it("returns null for an undated row", () => {
    expect(baseWindow({ startDate: null, targetDate: null })).toBeNull();
  });
});

describe("movedDates (ISS-051)", () => {
  it("shifts both ends by the same offset", () => {
    expect(movedDates({ startDate: "2026-10-01", targetDate: "2026-10-03" }, 3)).toEqual({
      startDate: "2026-10-04",
      targetDate: "2026-10-06",
    });
  });

  it("preserves the inclusive duration", () => {
    const before = { startDate: "2026-10-01", targetDate: "2026-10-03" };
    expect(windowDays(movedDates(before, -2)!)).toBe(windowDays(before));
  });

  it("refuses a half-dated row", () => {
    expect(movedDates({ startDate: "2026-10-01", targetDate: null }, 2)).toBeNull();
  });
});

describe("resizedDates (ISS-052/053)", () => {
  const row = { startDate: "2026-10-01", targetDate: "2026-10-05" };

  it("moves the left end and keeps the target", () => {
    expect(resizedDates(row, "left", 2)).toEqual({
      window: { startDate: "2026-10-03", targetDate: "2026-10-05" },
      write: { startDate: "2026-10-03", targetDate: "2026-10-05" },
    });
  });

  it("moves the right end and keeps the start", () => {
    expect(resizedDates(row, "right", 3)).toEqual({
      window: { startDate: "2026-10-01", targetDate: "2026-10-08" },
      write: { startDate: "2026-10-01", targetDate: "2026-10-08" },
    });
  });

  it("never narrows below one day", () => {
    expect(resizedDates(row, "left", 99)?.window).toEqual({ startDate: "2026-10-05", targetDate: "2026-10-05" });
    expect(resizedDates(row, "right", -99)?.window).toEqual({ startDate: "2026-10-01", targetDate: "2026-10-01" });
  });

  it("fills a missing start when the left handle drags it", () => {
    expect(resizedDates({ startDate: null, targetDate: "2026-10-05" }, "left", -1)).toEqual({
      window: { startDate: "2026-10-03", targetDate: "2026-10-05" },
      write: { startDate: "2026-10-03", targetDate: "2026-10-05" },
    });
  });

  it("fills a missing target when the right handle drags it", () => {
    expect(resizedDates({ startDate: "2026-10-01", targetDate: null }, "right", 2)).toEqual({
      window: { startDate: "2026-10-01", targetDate: "2026-10-04" },
      write: { startDate: "2026-10-01", targetDate: "2026-10-04" },
    });
  });

  it("does not invent the other end when the left handle fills a start", () => {
    // A start-only row dragged by its left handle keeps the missing target
    // missing, even though the preview draws a one-day window (ISS-052).
    expect(resizedDates({ startDate: null, targetDate: "2026-10-05" }, "left", -1)?.write.targetDate).toBe(
      "2026-10-05"
    );
    expect(resizedDates({ startDate: "2026-10-01", targetDate: null }, "left", 2)).toEqual({
      window: { startDate: "2026-10-02", targetDate: "2026-10-02" },
      write: { startDate: "2026-10-02", targetDate: null },
    });
  });

  it("does not invent the other end when the right handle fills a target", () => {
    expect(resizedDates({ startDate: "2026-10-01", targetDate: null }, "right", 2)?.write.startDate).toBe("2026-10-01");
    expect(resizedDates({ startDate: null, targetDate: "2026-10-05" }, "right", -2)).toEqual({
      window: { startDate: "2026-10-04", targetDate: "2026-10-04" },
      write: { startDate: null, targetDate: "2026-10-04" },
    });
  });

  it("returns null for an undated row", () => {
    expect(resizedDates({ startDate: null, targetDate: null }, "right", 2)).toBeNull();
  });
});

describe("handlePreviewDate", () => {
  it("previews the date under either handle", () => {
    const row = { startDate: "2026-10-01", targetDate: "2026-10-05" };
    expect(handlePreviewDate(row, "left")).toBe("2026-10-01");
    expect(handlePreviewDate(row, "right")).toBe("2026-10-05");
    expect(handlePreviewDate(row, "left", -2)).toBe("2026-09-29");
  });

  it("returns null for an undated row", () => {
    expect(handlePreviewDate({ startDate: null, targetDate: null }, "left")).toBeNull();
  });
});
