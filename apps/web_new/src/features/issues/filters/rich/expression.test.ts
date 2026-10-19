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
    ).toEqual({ and: [{ priority__in: "urgent" }, { not: { state_id__in: ["todo"] } }] });
  });

  it("round-trips the shapes the parser accepts", () => {
    const expression = { and: [{ priority__in: "urgent" }, { not: { state_id__in: ["todo"] } }] };
    expect(serializeRichExpression(parseRichExpression(expression))).toEqual(expression);
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
  it("offers the core fields this slice can edit", () => {
    expect(richFilterFields().map((field) => field.stem)).toEqual([
      "state_id",
      "state_group",
      "priority",
      "assignee_id",
      "created_by_id",
      "mention_id",
      "label_id",
    ]);
  });

  it("filters cycle and module when the flags hide them", () => {
    expect(richFilterFields({ hideCycle: true, hideModule: true })).toHaveLength(7);
    expect(richFilterFields({ hideCycle: false, hideModule: false })).toHaveLength(7);
    expect(richFilterFields()).toHaveLength(7);
  });

  it("drops exactly the dimension the surface hides", () => {
    const withDimensions = [...richFilterFields(), CYCLE_FIELD, MODULE_FIELD];
    expect(hideRichFilterFields(withDimensions, { hideCycle: true }).map((field) => field.stem)).not.toContain(
      "cycle_id"
    );
    expect(hideRichFilterFields(withDimensions, { hideModule: true }).map((field) => field.stem)).not.toContain(
      "module_id"
    );
    expect(hideRichFilterFields(withDimensions, {})).toHaveLength(9);
    expect(hideRichFilterFields(withDimensions, { hideCycle: false, hideModule: false })).toHaveLength(9);
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
