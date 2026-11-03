// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// MOD-034 model: the date-state rule and notices, the header share,
// the measure labels, the breakdown selectors, the burndown points,
// the per-module tab storage, and the entry filter toggle.

import { describe, expect, it } from "vitest";

import type { RichCondition } from "../../../issues/index.js";
import type { StorageLike } from "../../lib/listLayoutStore.js";
import {
  DEFAULT_MODULE_BREAKDOWN_TAB,
  MODULE_BREAKDOWN_FILTER_STEMS,
  MODULE_BREAKDOWN_TABS,
  MODULE_NO_ASSIGNEE_TITLE,
  MODULE_NO_LABELS_TITLE,
  MODULE_PROGRESS_INACTIVE_NOTICE,
  MODULE_PROGRESS_INVALID_NOTICE,
  MODULE_PROGRESS_MEASURES,
  MODULE_PROGRESS_TABS_STORAGE_KEY,
  moduleAssigneeEntries,
  moduleBreakdownEntryActive,
  moduleCompletionChartPresent,
  moduleCompletionPoints,
  moduleLabelEntries,
  moduleProgressDateState,
  moduleProgressGrandTotal,
  moduleProgressHeaderPercent,
  moduleProgressMeasureLabel,
  moduleProgressNotice,
  moduleProgressShare,
  parseModuleProgressTabRecord,
  readModuleProgressTab,
  readModuleProgressTabRecord,
  selectModuleAssigneeRows,
  selectModuleLabelRows,
  selectModuleStateGroupRows,
  toggleModuleBreakdownCondition,
  writeModuleProgressTab,
  type ModuleStateGroupCounts,
} from "./moduleProgressModel.js";

const TODAY = "2026-10-24";

function counts(overrides: Partial<ModuleStateGroupCounts> = {}): ModuleStateGroupCounts {
  return {
    backlog_issues: 1,
    unstarted_issues: 1,
    started_issues: 0,
    completed_issues: 1,
    cancelled_issues: 0,
    total_issues: 3,
    ...overrides,
  };
}

function memoryStorage(initial: Record<string, string> = {}): StorageLike {
  const record: Record<string, string> = { ...initial };
  return {
    getItem: (key: string) => record[key] ?? null,
    setItem: (key: string, value: string) => {
      record[key] = value;
    },
  };
}

describe("moduleProgressDateState", () => {
  it("opens for a started window", () => {
    expect(moduleProgressDateState("2026-09-01", "2026-12-31", TODAY)).toBe("active");
  });

  it("opens when the start is today and when the target is the start", () => {
    expect(moduleProgressDateState(TODAY, "2026-12-31", TODAY)).toBe("active");
    expect(moduleProgressDateState("2026-09-01", "2026-09-01", TODAY)).toBe("active");
  });

  it("opens for an overdue target that is not before the start", () => {
    expect(moduleProgressDateState("2026-09-01", "2026-10-01", TODAY)).toBe("active");
  });

  it("compares days inside one month", () => {
    expect(moduleProgressDateState("2026-10-01", "2026-10-20", TODAY)).toBe("active");
    expect(moduleProgressDateState("2026-10-25", "2026-10-30", TODAY)).toBe("inactive");
  });

  it("stays inactive for a future start", () => {
    expect(moduleProgressDateState("2026-11-01", "2026-11-30", TODAY)).toBe("inactive");
  });

  it("stays inactive when the start is after the target", () => {
    expect(moduleProgressDateState("2026-12-31", "2026-09-01", TODAY)).toBe("inactive");
  });

  it("reads invalid when either side is missing", () => {
    expect(moduleProgressDateState(null, "2026-12-31", TODAY)).toBe("invalid");
    expect(moduleProgressDateState("2026-09-01", null, TODAY)).toBe("invalid");
    expect(moduleProgressDateState(null, null, TODAY)).toBe("invalid");
    expect(moduleProgressDateState(undefined, undefined, TODAY)).toBe("invalid");
  });

  it("reads invalid for malformed days", () => {
    expect(moduleProgressDateState("2026-13-01", "2026-12-31", TODAY)).toBe("invalid");
    expect(moduleProgressDateState("2026-09-01", "not-a-day", TODAY)).toBe("invalid");
    expect(moduleProgressDateState("2026-09-01", "2026-12-31", "someday")).toBe("invalid");
  });

  it("defaults today to the local day", () => {
    expect(moduleProgressDateState("2000-01-01", "2000-01-02")).toBe("active");
    expect(moduleProgressDateState("2999-01-01", "2999-01-02")).toBe("inactive");
  });
});

describe("moduleProgressNotice", () => {
  it("explains inactive and invalid windows, and stays silent when active", () => {
    expect(moduleProgressNotice("inactive")).toBe(MODULE_PROGRESS_INACTIVE_NOTICE);
    expect(moduleProgressNotice("invalid")).toBe(MODULE_PROGRESS_INVALID_NOTICE);
    expect(moduleProgressNotice("active")).toBeNull();
  });

  it("pins the oracle sentences", () => {
    expect(MODULE_PROGRESS_INACTIVE_NOTICE).toBe("This module isn't active yet.");
    expect(MODULE_PROGRESS_INVALID_NOTICE).toBe("Invalid date. Please enter valid date.");
  });
});

describe("moduleProgressHeaderPercent", () => {
  it("rounds the completed share", () => {
    expect(moduleProgressHeaderPercent(1, 2)).toBe(50);
    expect(moduleProgressHeaderPercent(1, 3)).toBe(33);
    expect(moduleProgressHeaderPercent(2, 3)).toBe(67);
  });

  it("reads zero for missing or zero sides", () => {
    expect(moduleProgressHeaderPercent(0, 3)).toBe(0);
    expect(moduleProgressHeaderPercent(1, 0)).toBe(0);
    expect(moduleProgressHeaderPercent(undefined, 3)).toBe(0);
    expect(moduleProgressHeaderPercent(1, undefined)).toBe(0);
    expect(moduleProgressHeaderPercent(null, null)).toBe(0);
  });

  it("reads zero for unshaped or negative sides", () => {
    expect(moduleProgressHeaderPercent(Number.NaN, 3)).toBe(0);
    expect(moduleProgressHeaderPercent(1, Number.POSITIVE_INFINITY)).toBe(0);
    expect(moduleProgressHeaderPercent(-1, 3)).toBe(0);
    expect(moduleProgressHeaderPercent("1" as unknown as number, 3)).toBe(0);
  });
});

describe("measures", () => {
  it("labels the oracle options in order", () => {
    expect(MODULE_PROGRESS_MEASURES.map((entry) => entry.label)).toEqual(["Work items", "Points"]);
    expect(moduleProgressMeasureLabel("issues")).toBe("Work items");
    expect(moduleProgressMeasureLabel("points")).toBe("Points");
  });
});

describe("moduleProgressShare", () => {
  it("rounds the share and guards zero sides", () => {
    expect(moduleProgressShare(1, 3)).toBe(33);
    expect(moduleProgressShare(0, 3)).toBe(0);
    expect(moduleProgressShare(1, 0)).toBe(0);
    expect(moduleProgressShare(-1, 3)).toBe(0);
  });
});

describe("selectModuleStateGroupRows", () => {
  it("lists every group with its count and share of the grand total", () => {
    const rows = selectModuleStateGroupRows(counts(), "issues");
    expect(rows.map((row) => row.title)).toEqual(["Backlog", "Unstarted", "Started", "Completed", "Cancelled"]);
    expect(rows.map((row) => row.key)).toEqual(["backlog", "unstarted", "started", "completed", "cancelled"]);
    expect(rows.find((row) => row.key === "backlog")).toEqual({
      key: "backlog",
      title: "Backlog",
      count: 1,
      total: 3,
      percent: 33,
    });
    expect(rows.find((row) => row.key === "started")).toMatchObject({ count: 0, total: 3, percent: 0 });
  });

  it("reads estimate points under the points measure", () => {
    const rows = selectModuleStateGroupRows(
      counts({
        backlog_estimate_points: 2,
        unstarted_estimate_points: 0,
        started_estimate_points: 1,
        completed_estimate_points: 1,
        cancelled_estimate_points: 0,
        total_estimate_points: 4,
      }),
      "points"
    );
    expect(rows.find((row) => row.key === "backlog")).toMatchObject({ count: 2, total: 4, percent: 50 });
    expect(rows.find((row) => row.key === "completed")).toMatchObject({ count: 1, total: 4, percent: 25 });
  });

  it("reads missing estimate sums as zero", () => {
    const rows = selectModuleStateGroupRows(counts(), "points");
    expect(rows.find((row) => row.key === "backlog")).toMatchObject({ count: 0, total: 0, percent: 0 });
    expect(rows).toHaveLength(5);
  });
});

describe("moduleProgressGrandTotal", () => {
  it("reads issues or points", () => {
    expect(moduleProgressGrandTotal({ total_issues: 3, total_estimate_points: 4 }, "issues")).toBe(3);
    expect(moduleProgressGrandTotal({ total_issues: 3, total_estimate_points: 4 }, "points")).toBe(4);
    expect(moduleProgressGrandTotal({ total_issues: 3 }, "points")).toBe(0);
  });
});

describe("selectModuleAssigneeRows", () => {
  const ada = "aaaaaaaa-0000-4000-8000-000000000001";

  it("maps live rows with their own completed/total pair", () => {
    const rows = selectModuleAssigneeRows(
      [
        {
          assignee_id: ada,
          display_name: "Ada",
          total_issues: 1,
          completed_issues: 0,
          total_estimates: 2,
          completed_estimates: 0,
        },
        { assignee_id: null, display_name: null, total_issues: 2, completed_issues: 1 },
      ],
      "issues"
    );
    expect(rows).toEqual([
      { id: ada, title: "Ada", completed: 0, total: 1, percent: 0 },
      { id: null, title: MODULE_NO_ASSIGNEE_TITLE, completed: 1, total: 2, percent: 50 },
    ]);
    expect(MODULE_NO_ASSIGNEE_TITLE).toBe("No assignee");
  });

  it("reads estimate pairs under the points measure", () => {
    const rows = selectModuleAssigneeRows(
      [{ assignee_id: ada, display_name: "Ada", total_estimates: 2, completed_estimates: 2 }],
      "points"
    );
    expect(rows).toEqual([{ id: ada, title: "Ada", completed: 2, total: 2, percent: 100 }]);
  });

  it("coerces unshaped readings to zero and blank titles to the fallback", () => {
    const rows = selectModuleAssigneeRows(
      [
        {
          assignee_id: 7,
          display_name: "",
          total_issues: "3",
          completed_issues: Number.NaN,
          total_estimates: null,
          completed_estimates: undefined,
        },
      ],
      "issues"
    );
    expect(rows).toEqual([{ id: null, title: MODULE_NO_ASSIGNEE_TITLE, completed: 0, total: 0, percent: 0 }]);
  });
});

describe("selectModuleLabelRows", () => {
  const bug = "bbbbbbbb-0000-4000-8000-000000000002";

  it("maps live rows with their color", () => {
    const rows = selectModuleLabelRows(
      [
        { label_id: bug, label_name: "Bug", color: "#ff0000", total_issues: 1, completed_issues: 1 },
        { label_id: null, label_name: null, color: null, total_issues: 2, completed_issues: 0 },
      ],
      "issues"
    );
    expect(rows).toEqual([
      { id: bug, title: "Bug", color: "#ff0000", completed: 1, total: 1, percent: 100 },
      { id: null, title: MODULE_NO_LABELS_TITLE, color: undefined, completed: 0, total: 2, percent: 0 },
    ]);
    expect(MODULE_NO_LABELS_TITLE).toBe("No labels yet");
  });

  it("reads estimate pairs under the points measure", () => {
    const rows = selectModuleLabelRows(
      [{ label_id: bug, label_name: "Bug", color: "#ff0000", total_estimates: 3, completed_estimates: 1 }],
      "points"
    );
    expect(rows).toEqual([{ id: bug, title: "Bug", color: "#ff0000", completed: 1, total: 3, percent: 33 }]);
  });

  it("drops blank and unshaped colors", () => {
    const rows = selectModuleLabelRows(
      [
        { label_id: bug, label_name: "", color: "", total_issues: 1, completed_issues: 1 },
        { label_id: bug, label_name: "Bug", color: 7, total_issues: 1, completed_issues: 1 },
      ],
      "issues"
    );
    expect(rows[0]).toMatchObject({ title: MODULE_NO_LABELS_TITLE, color: undefined });
    expect(rows[1]).toMatchObject({ title: "Bug", color: undefined });
  });
});

describe("distribution entry readers", () => {
  it("reads each side and skips unshaped entries", () => {
    const distribution = {
      assignees: [{ assignee_id: null }, null, 7, { assignee_id: "a" }],
      labels: [{ label_id: "b" }],
    };
    expect(moduleAssigneeEntries(distribution)).toEqual([{ assignee_id: null }, { assignee_id: "a" }]);
    expect(moduleLabelEntries(distribution)).toEqual([{ label_id: "b" }]);
  });

  it("reads empty for missing or unshaped sides", () => {
    expect(moduleAssigneeEntries(undefined)).toEqual([]);
    expect(moduleLabelEntries(undefined)).toEqual([]);
    expect(moduleAssigneeEntries({})).toEqual([]);
    expect(moduleLabelEntries({ assignees: [], labels: "nope" })).toEqual([]);
  });
});

describe("moduleCompletionPoints", () => {
  it("maps dated readings oldest first with short labels", () => {
    expect(
      moduleCompletionPoints({
        completion_chart: { "2026-09-03": 1, "2026-09-01": 3, "2026-09-02": 2, "2026-09-04": null },
      })
    ).toEqual([
      { label: "Sep 1, 2026", values: { remaining: 3 } },
      { label: "Sep 2, 2026", values: { remaining: 2 } },
      { label: "Sep 3, 2026", values: { remaining: 1 } },
    ]);
  });

  it("drops unshaped readings and charts", () => {
    expect(moduleCompletionPoints(undefined)).toEqual([]);
    expect(moduleCompletionPoints({})).toEqual([]);
    expect(moduleCompletionPoints({ completion_chart: null })).toEqual([]);
    expect(moduleCompletionPoints({ completion_chart: [] })).toEqual([]);
    expect(moduleCompletionPoints({ completion_chart: "nope" })).toEqual([]);
    expect(moduleCompletionPoints({ completion_chart: { "2026-09-01": "3", "2026-09-02": Number.NaN } })).toEqual([]);
  });
});

describe("moduleCompletionChartPresent", () => {
  it("reads presence off the side object", () => {
    expect(moduleCompletionChartPresent({ completion_chart: {} })).toBe(true);
    expect(moduleCompletionChartPresent({ completion_chart: { "2026-09-01": 1 } })).toBe(true);
    expect(moduleCompletionChartPresent(undefined)).toBe(false);
    expect(moduleCompletionChartPresent({})).toBe(false);
    expect(moduleCompletionChartPresent({ completion_chart: null })).toBe(false);
    expect(moduleCompletionChartPresent({ completion_chart: [] })).toBe(false);
    expect(moduleCompletionChartPresent({ completion_chart: "nope" })).toBe(false);
  });
});

describe("tab storage", () => {
  it("defaults to Assignees and remembers per module", () => {
    const storage = memoryStorage();
    expect(MODULE_BREAKDOWN_TABS).toEqual(["States", "Assignees", "Labels"]);
    expect(DEFAULT_MODULE_BREAKDOWN_TAB).toBe("Assignees");
    expect(readModuleProgressTab("module-a", storage)).toBe("Assignees");
    writeModuleProgressTab("module-a", "Labels", storage);
    expect(readModuleProgressTab("module-a", storage)).toBe("Labels");
    expect(readModuleProgressTab("module-b", storage)).toBe("Assignees");
    writeModuleProgressTab("module-b", "States", storage);
    expect(readModuleProgressTab("module-a", storage)).toBe("Labels");
    expect(readModuleProgressTab("module-b", storage)).toBe("States");
  });

  it("drops corrupt records, unknown tabs and blank ids", () => {
    expect(parseModuleProgressTabRecord(null)).toEqual({});
    expect(parseModuleProgressTabRecord("{nope")).toEqual({});
    expect(parseModuleProgressTabRecord("[]")).toEqual({});
    expect(parseModuleProgressTabRecord("7")).toEqual({});
    expect(parseModuleProgressTabRecord(JSON.stringify({ a: "Labels", b: "stat-labels", "": "States", c: 7 }))).toEqual(
      { a: "Labels" }
    );
  });

  it("reads empty without storage or when the read throws", () => {
    expect(readModuleProgressTabRecord(undefined)).toEqual({});
    expect(readModuleProgressTab("module-a", undefined)).toBe("Assignees");
    const throwing = {
      getItem: () => {
        throw new Error("refused");
      },
      setItem: () => undefined,
    };
    expect(readModuleProgressTabRecord(throwing)).toEqual({});
    expect(readModuleProgressTab("module-a", throwing)).toBe("Assignees");
  });

  it("reads empty when the browser refuses storage", () => {
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      get() {
        throw new Error("refused");
      },
    });
    try {
      expect(readModuleProgressTabRecord()).toEqual({});
      expect(readModuleProgressTab("module-a")).toBe("Assignees");
      writeModuleProgressTab("module-a", "Labels");
    } finally {
      if (descriptor === undefined) delete (globalThis as Record<string, unknown>)["localStorage"];
      else Object.defineProperty(globalThis, "localStorage", descriptor);
    }
  });

  it("writes nothing without storage and survives a refused write", () => {
    writeModuleProgressTab("module-a", "Labels", undefined);
    const throwing = {
      getItem: () => null,
      setItem: () => {
        throw new Error("refused");
      },
    };
    writeModuleProgressTab("module-a", "Labels", throwing);
    expect(readModuleProgressTab("module-a", memoryStorage())).toBe("Assignees");
  });

  it("keeps other modules when writing", () => {
    const storage = memoryStorage({
      [MODULE_PROGRESS_TABS_STORAGE_KEY]: JSON.stringify({ a: "States" }),
    });
    writeModuleProgressTab("b", "Labels", storage);
    expect(readModuleProgressTabRecord(storage)).toEqual({ a: "States", b: "Labels" });
  });
});

describe("breakdown filter stems", () => {
  it("pins each tab's stem", () => {
    expect(MODULE_BREAKDOWN_FILTER_STEMS).toEqual({
      States: "state_group",
      Assignees: "assignee_id",
      Labels: "label_id",
    });
  });
});

describe("toggleModuleBreakdownCondition", () => {
  const backlog: RichCondition = {
    key: "k1",
    property: "state_group",
    operator: "in",
    negated: false,
    value: ["backlog"],
  };

  it("adds the first value to a fresh stem", () => {
    const next = toggleModuleBreakdownCondition([], "state_group", "backlog");
    expect(next).toHaveLength(1);
    expect(next[0]).toMatchObject({ property: "state_group", operator: "in", negated: false, value: ["backlog"] });
    expect(typeof next[0]?.key).toBe("string");
  });

  it("lifts the value on repeat, dropping the emptied condition", () => {
    expect(toggleModuleBreakdownCondition([backlog], "state_group", "backlog")).toEqual([]);
  });

  it("keeps the surviving values when one of several lifts", () => {
    const two: RichCondition = { ...backlog, value: ["backlog", "started"] };
    expect(toggleModuleBreakdownCondition([two], "state_group", "backlog")).toEqual([{ ...two, value: ["started"] }]);
  });

  it("adds to the stem and leaves other conditions untouched", () => {
    const assignee: RichCondition = {
      key: "k2",
      property: "assignee_id",
      operator: "in",
      negated: false,
      value: ["ada"],
    };
    const next = toggleModuleBreakdownCondition([backlog, assignee], "state_group", "started");
    expect(next).toHaveLength(2);
    expect(next[0]).toMatchObject({ property: "state_group", value: ["backlog", "started"] });
    expect(next[1]).toBe(assignee);
  });

  it("ignores negated and foreign-operator conditions on the same stem", () => {
    const negated: RichCondition = { ...backlog, key: "k9", negated: true };
    const next = toggleModuleBreakdownCondition([negated], "state_group", "started");
    expect(next).toHaveLength(2);
    expect(next[1]).toMatchObject({ property: "state_group", negated: false, value: ["started"] });
  });

  it("leaves unknown stems untouched", () => {
    expect(toggleModuleBreakdownCondition([backlog], "nope", "x")).toEqual([backlog]);
  });
});

describe("moduleBreakdownEntryActive", () => {
  it("reads the active marker off the stem condition", () => {
    const active: RichCondition = {
      key: "k1",
      property: "label_id",
      operator: "in",
      negated: false,
      value: ["bug"],
    };
    expect(moduleBreakdownEntryActive([active], "label_id", "bug")).toBe(true);
    expect(moduleBreakdownEntryActive([active], "label_id", "other")).toBe(false);
    expect(moduleBreakdownEntryActive([], "label_id", "bug")).toBe(false);
    expect(moduleBreakdownEntryActive([{ ...active, negated: true }], "label_id", "bug")).toBe(false);
  });
});
