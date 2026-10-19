// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  addCondition,
  conditionKey,
  findField,
  hasRichConditions,
  hideRichFilterFields,
  isOperatorLocked,
  isUnresolvableCondition,
  isValueComplete,
  isValueCompleteForOperator,
  normalizeRichExpression,
  operatorLabel,
  operatorOptions,
  parseRichExpression,
  removeCondition,
  replaceConditionProperty,
  richFilterFields,
  serializeRichExpression,
  updateCondition,
  usedProperties,
  valueLabel,
  type RichCondition,
  type RichFilterField,
} from "./expression.js";

const PRIORITY = findField(richFilterFields(), "priority") as RichFilterField;
const STATE = findField(richFilterFields(), "state_id") as RichFilterField;
const CYCLE_FIELD: RichFilterField = {
  stem: "cycle_id",
  label: "Cycle",
  kind: "multi" as const,
  defaultOperator: "in",
  defaultNegated: false,
  operators: ["in"],
  allowNegative: true,
};
const MODULE_FIELD: RichFilterField = { ...CYCLE_FIELD, stem: "module_id", label: "Module" };
const START = {
  stem: "start_date",
  label: "Start date",
  kind: "date_range" as const,
  defaultOperator: "range",
  defaultNegated: false,
  operators: ["exact", "range"],
  allowNegative: true,
};

function condition(overrides: Partial<RichCondition> = {}): RichCondition {
  return { key: conditionKey(), property: "priority", operator: "in", negated: false, value: "urgent", ...overrides };
}

describe("parseRichExpression", () => {
  it("reads an empty or non-record expression as no conditions", () => {
    expect(parseRichExpression({})).toEqual([]);
    expect(parseRichExpression(null)).toEqual([]);
    expect(parseRichExpression("priority__in")).toEqual([]);
  });

  it("reads one bare leaf", () => {
    expect(parseRichExpression({ priority__in: "urgent" })).toEqual([
      { key: "leaf-0", property: "priority", operator: "in", negated: false, value: "urgent" },
    ]);
  });

  it("normalizes a stored expression to the shape the builder writes", () => {
    expect(normalizeRichExpression({ and: [{ priority__in: "urgent" }] })).toEqual({ priority__in: "urgent" });
    expect(normalizeRichExpression({ state_id__in: "a,b" })).toEqual({ state_id__in: "a,b" });
    expect(normalizeRichExpression({ start_date__range: "2026-01-01,2026-01-31" })).toEqual({
      start_date__range: "2026-01-01,2026-01-31",
    });
    expect(normalizeRichExpression({ or: [] })).toEqual({});
    expect(normalizeRichExpression({})).toEqual({});
  });

  it("keeps keys stable across parses of the same expression", () => {
    const expression = { and: [{ priority__in: "urgent" }, { state_id__in: ["a"] }] };
    expect(parseRichExpression(expression).map((row) => row.key)).toEqual(
      parseRichExpression(expression).map((row) => row.key)
    );
  });

  it("flattens AND children in order", () => {
    expect(parseRichExpression({ and: [{ priority__in: "urgent" }, { state_id__in: ["a", "b"] }] })).toEqual([
      { key: "and-0", property: "priority", operator: "in", negated: false, value: "urgent" },
      { key: "and-1", property: "state_id", operator: "in", negated: false, value: ["a", "b"] },
    ]);
  });

  it("reads negation at the root and inside AND", () => {
    expect(parseRichExpression({ not: { priority__in: "urgent" } })).toEqual([
      { key: "not-0", property: "priority", operator: "in", negated: true, value: "urgent" },
    ]);
    expect(parseRichExpression({ and: [{ not: { priority__in: "urgent" } }, { state_group__in: "started" }] })).toEqual(
      [
        { key: "and-0", property: "priority", operator: "in", negated: true, value: "urgent" },
        { key: "and-1", property: "state_group", operator: "in", negated: false, value: "started" },
      ]
    );
  });

  it("keeps an unknown stored shape as an invalid condition", () => {
    expect(parseRichExpression({ or: [{ priority__in: "urgent" }] })).toMatchObject([{ property: "", operator: "" }]);
    expect(parseRichExpression({ and: "nope" })).toMatchObject([{ property: "" }]);
    expect(parseRichExpression({ and: [null] })).toMatchObject([{ key: "and-0", property: "" }]);
    expect(parseRichExpression({ not: "nope" })).toMatchObject([{ property: "" }]);
    expect(parseRichExpression({ not: { a: 1, b: 2 } })).toMatchObject([{ property: "" }]);
    expect(parseRichExpression({ and: [{ not: { a: 1, b: 2 } }] })).toMatchObject([{ property: "" }]);
    expect(parseRichExpression({ nolookup: 1 })).toMatchObject([{ property: "" }]);
  });

  it("keeps a nested non-leaf AND child as an invalid condition", () => {
    expect(parseRichExpression({ and: [{ and: [{ priority__in: "urgent" }] }] })).toMatchObject([
      { key: "and-0", property: "" },
    ]);
    expect(parseRichExpression({ and: [{ not: "nope" }] })).toMatchObject([{ key: "and-0", property: "" }]);
  });

  it("splits a stored CSV collection back into its picks", () => {
    expect(parseRichExpression({ assignee_id__in: "a,b,c" })[0]?.value).toEqual(["a", "b", "c"]);
    expect(parseRichExpression({ and: [{ label_id__in: "a, b" }, { priority__in: "urgent" }] })).toMatchObject([
      { property: "label_id", value: ["a", "b"] },
      { property: "priority", value: "urgent" },
    ]);
    expect(parseRichExpression({ not: { state_id__in: "a,b" } })[0]).toMatchObject({
      negated: true,
      value: ["a", "b"],
    });
  });

  it("leaves a scalar and a single exact value alone", () => {
    expect(parseRichExpression({ state_group__in: "started" })[0]?.value).toBe("started");
    expect(parseRichExpression({ priority__in: "urgent" })[0]?.value).toBe("urgent");
    expect(parseRichExpression({ assignee_id__exact: "a,b" })[0]?.value).toEqual(["a", "b"]);
    // A separator-only string has no entries to read back.
    expect(parseRichExpression({ assignee_id__in: "," })[0]?.value).toBe(",");
  });

  it("splits a stored date range back into its two ends", () => {
    expect(parseRichExpression({ start_date__range: "2026-01-01,2026-01-31" })[0]?.value).toEqual([
      "2026-01-01",
      "2026-01-31",
    ]);
    expect(parseRichExpression({ and: [{ not: { start_date__range: "2026-01-01,2026-01-31" } }] })[0]).toMatchObject({
      negated: true,
      value: ["2026-01-01", "2026-01-31"],
    });
    // A half-filled stored range keeps its raw string rather than becoming a pair.
    expect(parseRichExpression({ start_date__range: "2026-01-01," })[0]?.value).toBe("2026-01-01,");
  });

  it("round-trips a date range through serialize and parse", () => {
    const stored = { start_date__range: "2026-01-01,2026-01-31" };
    expect(serializeRichExpression(parseRichExpression(stored))).toEqual(stored);
  });

  it("round-trips a multi pick through serialize and parse", () => {
    const condition: RichCondition = {
      key: "a",
      property: "assignee_id",
      operator: "in",
      negated: false,
      value: ["a", "b", "c"],
    };
    const stored = serializeRichExpression([condition]);
    expect(stored).toEqual({ assignee_id__in: "a,b,c" });
    const parsed = parseRichExpression(stored);
    expect(parsed[0]?.value).toEqual(["a", "b", "c"]);
    expect(serializeRichExpression(parsed)).toEqual(stored);
  });

  it("reads a stored collection exact back under its collection lookup", () => {
    const [pick] = parseRichExpression({ state_id__exact: "todo" });
    expect(pick).toMatchObject({ property: "state_id", operator: "in", value: "todo" });
    // A one-item list from another surface reads as the same single pick.
    expect(parseRichExpression({ state_id__in: ["todo"] })[0]).toMatchObject({
      property: "state_id",
      operator: "in",
      value: ["todo"],
    });
    const [several] = parseRichExpression({ priority__exact: "urgent,high" });
    expect(several).toMatchObject({ property: "priority", operator: "in", value: ["urgent", "high"] });
    // A date field's exact lookup is a single day, not a collection pick.
    expect(parseRichExpression({ start_date__exact: "2026-02-10" })[0]?.operator).toBe("exact");
  });

  it("does not merge two operators on one leaf", () => {
    expect(parseRichExpression({ priority__in: "urgent", state__in: "todo" })).toMatchObject([{ property: "" }]);
  });
});

describe("serializeRichExpression", () => {
  it("writes an empty object when no condition is complete", () => {
    expect(serializeRichExpression([])).toEqual({});
    expect(serializeRichExpression([condition({ property: "", operator: "" })])).toEqual({});
    expect(serializeRichExpression([condition({ value: null })])).toEqual({});
    expect(serializeRichExpression([condition({ value: [] })])).toEqual({});
  });

  it("skips an incomplete draft while keeping complete conditions", () => {
    expect(
      serializeRichExpression([condition({ key: "a" }), condition({ key: "b", property: "state_id", value: null })])
    ).toEqual({ priority__in: "urgent" });
  });

  it("writes a lone positive leaf bare and a lone negative leaf as not", () => {
    expect(serializeRichExpression([condition()])).toEqual({ priority__in: "urgent" });
    expect(serializeRichExpression([condition({ negated: true })])).toEqual({ not: { priority__in: "urgent" } });
  });

  it("joins multiple conditions with AND, including negated leaves", () => {
    expect(
      serializeRichExpression([
        condition({ key: "a", property: "priority", value: "urgent" }),
        condition({ key: "b", property: "state_id", value: ["todo"], negated: true }),
      ])
    ).toEqual({ and: [{ priority__in: "urgent" }, { not: { state_id__in: "todo" } }] });
  });

  it("round-trips the shapes the parser accepts", () => {
    const expression = { and: [{ priority__in: "urgent" }, { not: { state_id__in: ["todo"] } }] };
    expect(serializeRichExpression(parseRichExpression(expression))).toEqual({
      and: [{ priority__in: "urgent" }, { not: { state_id__in: "todo" } }],
    });
  });

  it("sends collection picks as one comma-separated value", () => {
    expect(
      serializeRichExpression([condition({ property: "assignee_id", operator: "in", value: ["a", "b", "c"] })])
    ).toEqual({ assignee_id__in: "a,b,c" });
  });

  it("round-trips one pick and several through the collection lookup", () => {
    const one = { state_id__in: "todo" };
    expect(serializeRichExpression(parseRichExpression(one))).toEqual(one);
    const many = { state_id__in: "todo,in-progress" };
    expect(serializeRichExpression(parseRichExpression(many))).toEqual(many);
    // A stored one-pick exact reads back as a collection pick and re-saves
    // under `__in`, which accepts one id as well as many.
    const exact = { state_id__exact: "todo" };
    expect(serializeRichExpression(parseRichExpression(exact))).toEqual(one);
    // Growing a one-pick exact to two picks keeps a collection lookup.
    expect(
      serializeRichExpression([condition({ property: "priority", operator: "in", value: ["urgent", "high"] })])
    ).toEqual({ priority__in: "urgent,high" });
  });

  it("moves a scalar exact to the collection lookup once it holds a list", () => {
    expect(
      serializeRichExpression([condition({ property: "priority", operator: "exact", value: ["urgent", "high"] })])
    ).toEqual({ priority__in: "urgent,high" });
  });

  it("sends a date range as its two ends in order", () => {
    expect(
      serializeRichExpression([
        condition({ property: "start_date", operator: "range", value: ["2026-01-01", "2026-01-31"] }),
      ])
    ).toEqual({ start_date__range: "2026-01-01,2026-01-31" });
  });
});

describe("condition operations", () => {
  it("adds with the field defaults and a fresh key", () => {
    const added = addCondition([], PRIORITY);
    expect(added).toHaveLength(1);
    expect(added[0]).toMatchObject({ property: "priority", operator: "in", negated: false, value: null });
    expect(addCondition(added, STATE)[1]!.property).toBe("state_id");
  });

  it("updates immutably and keeps the key", () => {
    const row = condition({ key: "keep" });
    const next = updateCondition([row], "keep", { value: ["high", "low"], negated: true });
    expect(next[0]).toMatchObject({ key: "keep", value: ["high", "low"], negated: true });
    expect(row.value).toBe("urgent");
  });

  it("replaces a property and resets operator and value", () => {
    const row = condition({ key: "keep", value: "urgent", negated: true });
    const next = replaceConditionProperty([row], "keep", STATE);
    expect(next[0]).toMatchObject({ key: "keep", property: "state_id", operator: "in", negated: false, value: null });
  });

  it("leaves other rows untouched when replacing a property", () => {
    const other = condition({ key: "other", property: "state_group" });
    const row = condition({ key: "keep" });
    expect(replaceConditionProperty([row, other], "keep", STATE)[1]).toBe(other);
  });

  it("removes one condition and reports the used stems", () => {
    const row = condition({ key: "a" });
    const other = condition({ key: "b", property: "state_id" });
    expect(removeCondition([row, other], "a")).toEqual([other]);
    expect(usedProperties([row, other, condition({ property: "" })])).toEqual(new Set(["priority", "state_id"]));
  });

  it("detects conditions and unresolvable fields", () => {
    expect(hasRichConditions([])).toBe(false);
    expect(hasRichConditions([condition()])).toBe(true);
    expect(isUnresolvableCondition(condition({ property: "gone" }), richFilterFields())).toBe(true);
    expect(isUnresolvableCondition(condition({ property: "" }), richFilterFields())).toBe(true);
    expect(isUnresolvableCondition(condition(), richFilterFields())).toBe(false);
  });
});

describe("field catalogue", () => {
  it("offers the core fields plus the value-input fields this slice edits", () => {
    expect(richFilterFields().map((field) => field.stem)).toEqual([
      "state_id",
      "state_group",
      "priority",
      "assignee_id",
      "created_by_id",
      "mention_id",
      "label_id",
      "project_id",
      "cycle_id",
      "module_id",
      "target_date",
      "start_date",
    ]);
  });

  it("filters cycle and module when the flags hide them", () => {
    const all = richFilterFields();
    expect(richFilterFields({ hideCycle: true, hideModule: true })).toHaveLength(all.length - 2);
    expect(richFilterFields({ hideCycle: false, hideModule: false })).toHaveLength(all.length);
    expect(richFilterFields()).toHaveLength(all.length);
  });

  it("uses the field kinds the old filter set offers", () => {
    // Every id/static dimension is a multi pick (one pick still stores
    // under exact); the dates are a single day plus a range.
    for (const stem of [
      "state_id",
      "state_group",
      "priority",
      "assignee_id",
      "created_by_id",
      "mention_id",
      "label_id",
      "project_id",
      "cycle_id",
      "module_id",
    ]) {
      expect(findField(richFilterFields(), stem)?.kind, stem).toBe("multi");
    }
    expect(findField(richFilterFields(), "target_date")?.kind).toBe("date");
    expect(findField(richFilterFields(), "start_date")?.kind).toBe("date_range");
  });

  it("carries optional bounds for the date kinds", () => {
    const bounded: RichFilterField = {
      ...(findField(richFilterFields(), "target_date") as RichFilterField),
      min: "2026-01-01",
      max: "2026-12-31",
    };
    expect(bounded.min).toBe("2026-01-01");
    expect(bounded.max).toBe("2026-12-31");
  });

  it("drops exactly the dimension the surface hides", () => {
    const withDimensions = [...richFilterFields(), CYCLE_FIELD, MODULE_FIELD];
    expect(hideRichFilterFields(withDimensions, { hideCycle: true }).map((field) => field.stem)).not.toContain(
      "cycle_id"
    );
    expect(hideRichFilterFields(withDimensions, { hideModule: true }).map((field) => field.stem)).not.toContain(
      "module_id"
    );
    expect(hideRichFilterFields(withDimensions, {})).toHaveLength(withDimensions.length);
    expect(hideRichFilterFields(withDimensions, { hideCycle: false, hideModule: false })).toHaveLength(
      withDimensions.length
    );
  });

  it("labels the state group picker entry and names each value source", () => {
    const fields = richFilterFields();
    expect(findField(fields, "state_group")?.label).toBe("State Group");
    expect(findField(fields, "state_id")?.source).toBe("states");
    for (const stem of ["assignee_id", "created_by_id", "mention_id"]) {
      expect(findField(fields, stem)).toMatchObject({ source: "members", kind: "multi", operators: ["in"] });
    }
    expect(findField(fields, "label_id")).toMatchObject({ source: "labels", kind: "multi", operators: ["in"] });
    expect(findField(fields, "priority")?.source).toBeUndefined();
  });

  it("exposes static values and the priority order", () => {
    const priority = findField(richFilterFields(), "priority")!;
    expect(priority.staticValues?.map((entry) => entry.value)).toEqual(["urgent", "high", "medium", "low", "none"]);
    expect(findField(richFilterFields(), "state_group")!.staticValues).toHaveLength(5);
  });

  it("returns null for an unknown stem", () => {
    expect(findField(richFilterFields(), "missing")).toBeNull();
  });
});

describe("operators", () => {
  it("offers positive and negated forms", () => {
    expect(operatorOptions(PRIORITY)).toEqual([
      { base: "in", negated: false, label: "is" },
      { base: "in", negated: true, label: "is not" },
    ]);
  });

  it("offers the negation pair and leaves the operator menu unlocked", () => {
    expect(isOperatorLocked(PRIORITY)).toBe(false);
    expect(isOperatorLocked(START)).toBe(false);
    expect(operatorOptions(START).map((option) => option.label)).toEqual(["is", "is not", "between", "not between"]);
  });

  it("locks a field that declares no negated form", () => {
    const locked = { ...PRIORITY, allowNegative: false };
    expect(operatorOptions(locked)).toEqual([{ base: "in", negated: false, label: "is" }]);
    expect(isOperatorLocked(locked)).toBe(true);
  });

  it("labels the stored operator and its negation", () => {
    expect(operatorLabel("exact", false)).toBe("is");
    expect(operatorLabel("exact", true)).toBe("is not");
    expect(operatorLabel("range", true)).toBe("not between");
    expect(operatorLabel("custom", false)).toBe("custom");
    expect(operatorLabel("custom", true)).toBe("not custom");
  });

  it("falls back to the raw lookup for an operator with no label", () => {
    expect(operatorOptions({ ...PRIORITY, operators: ["custom"] })[0]).toEqual({
      base: "custom",
      negated: false,
      label: "custom",
    });
  });
});

describe("value completeness and labels", () => {
  it("requires both ends for a date range", () => {
    expect(isValueComplete("date_range", null)).toBe(false);
    expect(isValueComplete("date_range", ["2026-01-01"])).toBe(false);
    expect(isValueComplete("date_range", ["2026-01-01", "2026-01-02"])).toBe(true);
    expect(isValueComplete("date_range", ["", "2026-01-02"])).toBe(false);
  });

  it("judges a date field by its operator's value shape", () => {
    const start = findField(richFilterFields(), "start_date")!;
    const target = findField(richFilterFields(), "target_date")!;
    expect(isValueCompleteForOperator(start, "exact", "2026-01-01")).toBe(true);
    expect(isValueCompleteForOperator(start, "exact", null)).toBe(false);
    expect(isValueCompleteForOperator(start, "exact", ["2026-01-01", "2026-01-02"])).toBe(false);
    expect(isValueCompleteForOperator(start, "range", ["2026-01-01", "2026-01-02"])).toBe(true);
    expect(isValueCompleteForOperator(start, "range", "2026-01-01,2026-01-02")).toBe(false);
    expect(isValueCompleteForOperator(start, "range", ["2026-01-01"])).toBe(false);
    expect(isValueCompleteForOperator(target, "exact", "2026-01-01")).toBe(true);
    expect(isValueCompleteForOperator(target, "range", ["2026-01-01", "2026-01-02"])).toBe(true);
  });

  it("falls back to the kind for lookups that do not change the shape", () => {
    const priority = findField(richFilterFields(), "priority")!;
    const start = findField(richFilterFields(), "start_date")!;
    expect(isValueCompleteForOperator(priority, "in", ["urgent"])).toBe(true);
    expect(isValueCompleteForOperator(priority, "in", [])).toBe(false);
    expect(isValueCompleteForOperator(start, "in", ["2026-01-01", "2026-01-02"])).toBe(true);
  });

  it("requires one or more values for multi and a value for date/single", () => {
    expect(isValueComplete("multi", [])).toBe(false);
    expect(isValueComplete("multi", ["a"])).toBe(true);
    expect(isValueComplete("date", "")).toBe(false);
    expect(isValueComplete("date", "2026-01-01")).toBe(true);
    expect(isValueComplete("single", null)).toBe(false);
    expect(isValueComplete("single", "x")).toBe(true);
  });

  it("labels static values and joins more than one", () => {
    expect(valueLabel(PRIORITY, ["urgent", "high"])).toBe("Urgent, High");
    expect(valueLabel(PRIORITY, "none")).toBe("None");
    expect(valueLabel(PRIORITY, [])).toBeNull();
    expect(valueLabel(PRIORITY, ["missing"])).toBeNull();
  });

  it("treats a non-string stored value as unresolvable", () => {
    expect(valueLabel(PRIORITY, [1])).toBeNull();
    expect(valueLabel(PRIORITY, 1)).toBeNull();
  });

  it("labels dates and ranges", () => {
    const { staticValues: _ignored, ...dateField } = PRIORITY;
    expect(valueLabel({ ...dateField, kind: "date" }, "2026-01-01")).toBe("2026-01-01");
    expect(valueLabel({ ...START, kind: "date_range" }, ["2026-01-01", "2026-01-02"])).toBe("2026-01-01 → 2026-01-02");
    expect(valueLabel({ ...START, kind: "date_range" }, "2026-01-01")).toBeNull();
  });
});
