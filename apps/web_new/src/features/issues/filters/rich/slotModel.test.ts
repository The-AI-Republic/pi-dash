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
  rangeEndsFromString,
  rangeValue,
  selectedIds,
  slotEditor,
  slotSource,
  togglePick,
  valueForOperator,
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
    expect(slotEditor(field("target_date"), "exact")).toBe("date");
    expect(slotEditor(field("start_date"), "range")).toBe("range");
    expect(slotEditor(field("priority"), "in")).toBe("choice");
    expect(slotEditor(field("assignee_id"), "in")).toBe("pick");
    expect(slotEditor(field("state_id"), "in")).toBe("pick");
  });

  it("edits a start date as one day under its exact lookup and a pair under range", () => {
    expect(slotEditor(field("start_date"), "exact")).toBe("date");
    expect(slotEditor(field("start_date"), "range")).toBe("range");
    // A date-only field keeps its single-day editor under every lookup.
    expect(slotEditor(field("target_date"), "range")).toBe("range");
  });

  it("falls back to the kind's editor for any other lookup", () => {
    expect(slotEditor(field("start_date"), "in")).toBe("range");
    expect(slotEditor(field("target_date"), "in")).toBe("date");
  });
});

describe("opensWhenIncomplete", () => {
  it("opens an empty slot, static lists included, and keeps a filled one closed", () => {
    expect(opensWhenIncomplete(false)).toBe(true);
    expect(opensWhenIncomplete(true)).toBe(false);
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

  it("marks a half-filled range incomplete", () => {
    expect(isRangeIncomplete(["2026-01-01", null])).toBe(true);
    expect(isRangeIncomplete([null, "2026-01-01"])).toBe(true);
    expect(isRangeIncomplete(["a", "b"])).toBe(false);
    expect(isRangeIncomplete(null)).toBe(false);
    expect(isRangeIncomplete("2026-01-01,2026-01-05")).toBe(false);
    expect(isRangeIncomplete("2026-01-01,")).toBe(true);
    expect(isRangeIncomplete({ start: "2026-01-01", end: null })).toBe(true);
    expect(isRangeIncomplete({ start: null, end: null })).toBe(false);
  });

  it("reads a stored range string back into its two ends", () => {
    expect(rangeEndsFromString("2026-01-01,2026-01-05")).toEqual({ start: "2026-01-01", end: "2026-01-05" });
    expect(rangeEndsFromString("2026-01-01,")).toEqual({ start: "2026-01-01", end: null });
    expect(rangeEndsFromString("2026-01-01")).toEqual({ start: "2026-01-01", end: null });
    expect(rangeEndsFromString(["2026-01-01", null])).toEqual({ start: "2026-01-01", end: null });
    expect(rangeEndsFromString(null)).toEqual({ start: null, end: null });
  });
});

describe("valueForOperator", () => {
  it("keeps the value when both lookups share a shape", () => {
    expect(valueForOperator("in", "in", ["a"])).toEqual(["a"]);
    expect(valueForOperator("exact", "exact", "2026-01-01")).toBe("2026-01-01");
    expect(valueForOperator("range", "range", ["2026-01-01", "2026-01-02"])).toEqual(["2026-01-01", "2026-01-02"]);
  });

  it("carries a day into the first end when narrowing or widening", () => {
    expect(valueForOperator("exact", "range", "2026-01-01")).toEqual(["2026-01-01", null]);
    expect(valueForOperator("range", "exact", ["2026-01-01", "2026-01-05"])).toBe("2026-01-01");
    expect(valueForOperator("range", "exact", "2026-01-01,2026-01-05")).toBe("2026-01-01");
    expect(valueForOperator("exact", "range", null)).toBeNull();
    expect(valueForOperator("range", "exact", null)).toBeNull();
  });

  it("leaves a non-date lookup's value alone", () => {
    expect(valueForOperator("in", "exact", "a")).toBe("a");
    expect(valueForOperator("exact", "in", ["a"])).toEqual(["a"]);
  });

  it("drops a value that cannot become a range", () => {
    expect(valueForOperator("in", "range", null)).toBeNull();
    expect(valueForOperator("in", "range", ["a", "b"])).toEqual(["a", "b"]);
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
