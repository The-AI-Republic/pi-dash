// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { findField, richFilterFields, type RichFilterField } from "./expression.js";
import {
  EMPTY_SLOT_LABEL,
  isRangeIncomplete,
  opensWhenIncomplete,
  pickTriggerLabel,
  rangeEnds,
  rangeLabel,
  rangeValue,
  selectedIds,
  slotEditor,
  slotSource,
  togglePick,
} from "./slotModel.js";

const FIELDS = richFilterFields();
const field = (stem: string): RichFilterField => findField(FIELDS, stem)!;

describe("rangeLabel", () => {
  it("names both ends, one open end or nothing", () => {
    expect(rangeLabel({ start: "2026-02-10", end: "2026-02-12" })).toBe("2026-02-10 → 2026-02-12");
    expect(rangeLabel({ start: "2026-02-10", end: null })).toBe("2026-02-10 → …");
    expect(rangeLabel({ start: null, end: "2026-02-12" })).toBe(EMPTY_SLOT_LABEL);
    expect(rangeLabel({ start: null, end: null })).toBe(EMPTY_SLOT_LABEL);
  });
});

describe("slotSource", () => {
  it("maps each entity-backed stem to its read", () => {
    expect(slotSource(field("state_id"))).toBe("states");
    expect(slotSource(field("assignee_id"))).toBe("members");
    expect(slotSource(field("created_by_id"))).toBe("members");
    expect(slotSource(field("mention_id"))).toBe("members");
    expect(slotSource(field("label_id"))).toBe("labels");
    expect(slotSource(field("cycle_id"))).toBe("cycles");
    expect(slotSource(field("module_id"))).toBe("modules");
    expect(slotSource(field("project_id"))).toBe("projects");
  });

  it("leaves fixed-level fields static", () => {
    expect(slotSource(field("priority"))).toBe("static");
    expect(slotSource(field("state_group"))).toBe("static");
  });
});

describe("slotEditor", () => {
  it("picks calendars for date kinds and lists for static ones", () => {
    expect(slotEditor(field("target_date"))).toBe("date");
    expect(slotEditor(field("start_date"))).toBe("range");
    expect(slotEditor(field("priority"))).toBe("choice");
    expect(slotEditor(field("assignee_id"))).toBe("pick");
    expect(slotEditor(field("state_id"))).toBe("pick");
  });
});

describe("opensWhenIncomplete", () => {
  it("never auto-opens a static choice list", () => {
    expect(opensWhenIncomplete(field("priority"), null, false)).toBe(false);
  });

  it("opens when a pick or a calendar holds no complete value", () => {
    expect(opensWhenIncomplete(field("state_id"), null, false)).toBe(true);
    expect(opensWhenIncomplete(field("state_id"), "todo", true)).toBe(false);
    expect(opensWhenIncomplete(field("target_date"), null, false)).toBe(true);
    expect(opensWhenIncomplete(field("target_date"), "2026-01-02", true)).toBe(false);
  });
});

describe("selectedIds", () => {
  it("reads arrays, ignores non-strings, and treats a lone string as one pick", () => {
    expect(selectedIds(["a", 1, "b"])).toEqual(["a", "b"]);
    expect(selectedIds("a")).toEqual(["a"]);
    expect(selectedIds("")).toEqual([]);
    expect(selectedIds(null)).toEqual([]);
    expect(selectedIds(3)).toEqual([]);
  });
});

describe("range helpers", () => {
  it("reads both ends and tolerates a start-only range", () => {
    expect(rangeEnds(["2026-01-01", "2026-01-05"])).toEqual({ start: "2026-01-01", end: "2026-01-05" });
    expect(rangeEnds(["2026-01-01"])).toEqual({ start: "2026-01-01", end: null });
    expect(rangeEnds([null, "2026-01-05"])).toEqual({ start: null, end: "2026-01-05" });
    expect(rangeEnds(null)).toEqual({ start: null, end: null });
  });

  it("stores both ends and maps an empty pair to no value", () => {
    expect(rangeValue("2026-01-01", "2026-01-05")).toEqual(["2026-01-01", "2026-01-05"]);
    expect(rangeValue("2026-01-01", null)).toEqual(["2026-01-01", null]);
    expect(rangeValue(null, null)).toBeNull();
  });

  it("marks a half-filled range incomplete and only for range kinds", () => {
    expect(isRangeIncomplete(field("start_date"), ["2026-01-01", null])).toBe(true);
    expect(isRangeIncomplete(field("start_date"), [null, "2026-01-01"])).toBe(true);
    expect(isRangeIncomplete(field("start_date"), ["a", "b"])).toBe(false);
    expect(isRangeIncomplete(field("start_date"), null)).toBe(false);
    expect(isRangeIncomplete(field("target_date"), ["2026-01-01", null])).toBe(false);
  });
});

describe("togglePick", () => {
  it("adds, removes and clears picks", () => {
    expect(togglePick(field("priority"), null, "urgent")).toEqual(["urgent"]);
    expect(togglePick(field("priority"), ["urgent"], "high")).toEqual(["urgent", "high"]);
    expect(togglePick(field("priority"), ["urgent", "high"], "urgent")).toEqual(["high"]);
    expect(togglePick(field("priority"), ["urgent"], "urgent")).toBeNull();
  });

  it("keeps a single field to one value, replacing on a new pick and clearing on the same one", () => {
    expect(togglePick(field("target_date"), "2026-01-01", "b")).toBe("b");
    expect(togglePick(field("target_date"), "2026-01-01", "2026-01-01")).toBeNull();
    expect(togglePick(field("assignee_id"), ["a"], "b")).toEqual(["a", "b"]);
  });
});

describe("pickTriggerLabel", () => {
  it("shows the dash when empty, both labels up to two, then a count", () => {
    expect(pickTriggerLabel([])).toBe(EMPTY_SLOT_LABEL);
    expect(pickTriggerLabel(["Todo"])).toBe("Todo");
    expect(pickTriggerLabel(["Todo", "Done"])).toBe("Todo, Done");
    expect(pickTriggerLabel(["a", "b", "c"])).toBe("a, b +1 more");
    expect(pickTriggerLabel(["a", "b", "c", "d"])).toBe("a, b +2 more");
  });
});
