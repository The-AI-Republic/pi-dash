// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules timeline model (parity/specs/modules.md §6, MOD-021): complete
// ranges plot and incomplete ones do not, bars take the status color,
// tooltips pair the name with the date summary, reorders normalize every
// visible rank, and the cache paints touch only the rows they name.
import { describe, expect, it } from "vitest";

import { makeModuleRow } from "../../testing.js";
import {
  buildModuleTimelineRows,
  changedModuleTimelineRanks,
  MODULE_TIMELINE_REORDER_FAILED_BODY,
  MODULE_TIMELINE_REORDER_FAILED_TITLE,
  MODULE_TIMELINE_REORDER_WARNING_BODY,
  MODULE_TIMELINE_REORDER_WARNING_TITLE,
  moduleTimelineBarColors,
  moduleTimelineBarTitle,
  moduleTimelinePrefixStatus,
  moduleTimelineRange,
  moduleTimelineTooltip,
  moduleTimelineTooltipDay,
  moduleTimelineTooltipForId,
  normalizeModuleTimelineRanks,
  warnOnTimelineReorderAttempt,
  withModuleTimelineDates,
  withModuleTimelineRanks,
} from "./moduleTimelineModel.js";

describe("moduleTimelineRange", () => {
  it("plots a module with both dates set", () => {
    expect(moduleTimelineRange({ start_date: "2026-09-01", target_date: "2026-09-18" })).toEqual({
      startIso: "2026-09-01",
      targetIso: "2026-09-18",
    });
  });

  it("plots nothing when an end is missing", () => {
    expect(moduleTimelineRange({ start_date: null, target_date: "2026-09-18" })).toBeNull();
    expect(moduleTimelineRange({ start_date: "2026-09-01", target_date: null })).toBeNull();
    expect(moduleTimelineRange({ start_date: null, target_date: null })).toBeNull();
  });

  it("treats a blank end as missing", () => {
    expect(moduleTimelineRange({ start_date: "", target_date: "2026-09-18" })).toBeNull();
    expect(moduleTimelineRange({ start_date: "2026-09-01", target_date: "" })).toBeNull();
  });
});

describe("buildModuleTimelineRows", () => {
  it("maps a dated module onto a chart row with its inclusive length", () => {
    const [row] = buildModuleTimelineRows([
      makeModuleRow({ id: "m-1", name: "Planned Bar", start_date: "2026-09-01", target_date: "2026-09-03" }),
    ]);
    expect(row).toMatchObject({
      id: "m-1",
      name: "Planned Bar",
      startIso: "2026-09-01",
      targetIso: "2026-09-03",
      durationDays: 3,
      sortOrder: 0,
      archived: false,
      unconfirmed: false,
      isEpic: false,
    });
  });

  it("keeps the row but drops the dates for an incomplete range", () => {
    const rows = buildModuleTimelineRows([
      makeModuleRow({ id: "m-1", start_date: "2026-09-01", target_date: null }),
      makeModuleRow({ id: "m-2", start_date: null, target_date: null }),
    ]);
    expect(rows.map((row) => [row.id, row.startIso, row.targetIso, row.durationDays])).toEqual([
      ["m-1", null, null, null],
      ["m-2", null, null, null],
    ]);
  });

  it("marks archived rows from the archive marker", () => {
    const rows = buildModuleTimelineRows([
      makeModuleRow({ id: "m-1", archived_at: "2026-10-01T00:00:00Z" }),
      makeModuleRow({ id: "m-2", archived_at: null }),
      makeModuleRow({ id: "m-3" }),
    ]);
    expect(rows.map((row) => row.archived)).toEqual([true, false, false]);
  });
});

describe("moduleTimelineBarColors", () => {
  it("colors every bar by its module status", () => {
    expect(
      moduleTimelineBarColors([
        makeModuleRow({ id: "m-1", status: "planned" }),
        makeModuleRow({ id: "m-2", status: "completed" }),
      ])
    ).toEqual({ "m-1": "#3f76ff", "m-2": "#16a34a" });
  });
});

describe("moduleTimelineTooltip", () => {
  it("pairs the name with the formatted date summary", () => {
    expect(moduleTimelineTooltip({ name: "Planned Bar", start_date: "2026-09-01", target_date: "2026-09-18" })).toBe(
      "Planned Bar: Sep 01, 2026 to Sep 18, 2026"
    );
  });

  it("reads nothing for a module without a bar", () => {
    expect(moduleTimelineTooltip({ name: "Dateless", start_date: null, target_date: null })).toBeNull();
  });

  it("reads an unparseable stored day back verbatim", () => {
    expect(moduleTimelineTooltipDay("2026-13-99")).toBe("2026-13-99");
    expect(moduleTimelineTooltipDay("2026-09-01")).toBe("Sep 01, 2026");
  });
});

describe("moduleTimelineBarTitle", () => {
  it("reads the tooltip for a dated row and the bare name otherwise", () => {
    const modules = new Map([
      ["m-1", makeModuleRow({ id: "m-1", name: "Planned Bar", start_date: "2026-09-01", target_date: "2026-09-18" })],
      ["m-2", makeModuleRow({ id: "m-2", name: "Dateless" })],
    ]);
    expect(moduleTimelineBarTitle(modules, { id: "m-1", name: "Planned Bar" })).toBe(
      "Planned Bar: Sep 01, 2026 to Sep 18, 2026"
    );
    expect(moduleTimelineBarTitle(modules, { id: "m-2", name: "Dateless" })).toBe("Dateless");
    expect(moduleTimelineBarTitle(modules, { id: "missing", name: "Missing" })).toBe("Missing");
  });
});

describe("moduleTimelineTooltipForId", () => {
  it("resolves the tooltip for a known dated row", () => {
    const modules = new Map([
      ["m-1", makeModuleRow({ id: "m-1", name: "Planned Bar", start_date: "2026-09-01", target_date: "2026-09-18" })],
    ]);
    expect(moduleTimelineTooltipForId(modules, "m-1")).toBe("Planned Bar: Sep 01, 2026 to Sep 18, 2026");
  });

  it("reads nothing for an unknown or dateless row", () => {
    const modules = new Map([["m-1", makeModuleRow({ id: "m-1" })]]);
    expect(moduleTimelineTooltipForId(modules, "missing")).toBeNull();
    expect(moduleTimelineTooltipForId(modules, "m-1")).toBeNull();
  });
});

describe("moduleTimelinePrefixStatus", () => {
  it("reads the module status and falls back to Backlog", () => {
    const modules = new Map([["m-1", makeModuleRow({ id: "m-1", status: "paused" })]]);
    expect(moduleTimelinePrefixStatus(modules, "m-1")).toBe("paused");
    expect(moduleTimelinePrefixStatus(modules, "missing")).toBe("backlog");
  });
});

describe("normalizeModuleTimelineRanks", () => {
  it("assigns distinct ascending ranks in the new order", () => {
    expect([...normalizeModuleTimelineRanks(["a", "b", "c"]).entries()]).toEqual([
      ["a", 1000],
      ["b", 2000],
      ["c", 3000],
    ]);
  });
});

describe("changedModuleTimelineRanks", () => {
  it("keeps only the rows whose rank moves", () => {
    const ranks = new Map([
      ["a", 1000],
      ["b", 2000],
      ["c", 3000],
    ]);
    expect(
      changedModuleTimelineRanks(
        [
          { id: "a", sort_order: 1000 },
          { id: "b", sort_order: 0 },
          { id: "c", sort_order: 0 },
        ],
        ranks
      )
    ).toEqual([
      { id: "b", sort_order: 2000 },
      { id: "c", sort_order: 3000 },
    ]);
  });

  it("skips rows the order does not name", () => {
    expect(changedModuleTimelineRanks([{ id: "a", sort_order: 0 }], new Map())).toEqual([]);
  });
});

describe("withModuleTimelineDates", () => {
  it("paints one row's dates and keeps every other row", () => {
    const rows = [
      makeModuleRow({ id: "m-1", start_date: "2026-09-01", target_date: "2026-09-03" }),
      makeModuleRow({ id: "m-2" }),
    ];
    const painted = withModuleTimelineDates(rows, "m-1", { startDate: "2026-09-04", targetDate: "2026-09-06" });
    expect(painted).toEqual([{ ...rows[0], start_date: "2026-09-04", target_date: "2026-09-06" }, rows[1]]);
  });

  it("passes a non-list cache entry through untouched", () => {
    expect(withModuleTimelineDates(undefined, "m-1", { startDate: null, targetDate: null })).toBeUndefined();
    expect(withModuleTimelineDates({ results: [] }, "m-1", { startDate: null, targetDate: null })).toEqual({
      results: [],
    });
  });
});

describe("withModuleTimelineRanks", () => {
  it("paints the written ranks and keeps unlisted rows", () => {
    const rows = [makeModuleRow({ id: "m-1", sort_order: 0 }), makeModuleRow({ id: "m-2", sort_order: 0 })];
    const painted = withModuleTimelineRanks(rows, [{ id: "m-2", sort_order: 2000 }]);
    expect(painted).toEqual([{ ...rows[0] }, { ...rows[1], sort_order: 2000 }]);
  });

  it("passes a non-list cache entry through untouched", () => {
    expect(withModuleTimelineRanks(undefined, [])).toBeUndefined();
  });
});

describe("warnOnTimelineReorderAttempt", () => {
  it("warns editors outside manual placement and stays silent otherwise", () => {
    expect(warnOnTimelineReorderAttempt(true, "name")).toBe(true);
    expect(warnOnTimelineReorderAttempt(true, "manual")).toBe(false);
    expect(warnOnTimelineReorderAttempt(false, "name")).toBe(false);
    expect(warnOnTimelineReorderAttempt(false, "manual")).toBe(false);
  });

  it("names manual sort in the warning copy", () => {
    expect(MODULE_TIMELINE_REORDER_WARNING_TITLE).toContain("Manual");
    expect(MODULE_TIMELINE_REORDER_WARNING_BODY).toContain("Manual");
    expect(MODULE_TIMELINE_REORDER_FAILED_TITLE).not.toBe("");
    expect(MODULE_TIMELINE_REORDER_FAILED_BODY).not.toBe("");
  });
});
