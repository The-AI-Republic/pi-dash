// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Rich-filter expression model (parity/specs/issues.md §9, ISS-095–099).
// Stored expressions are JSON filter trees: leaves name a field, operator
// and value, `and` joins conditions, and `not` negates one leaf. The
// builder edits a flat list of conditions and serializes back to the same
// shape, so the list query and the saved-view contract consume it directly.

import type { PriorityLevel } from "../../../../shared/pickers/priorities.js";

/** Field kinds the core builder can represent. Value editing arrives with ISS-100–103. */
export type RichFilterKind = "single" | "multi" | "date" | "date_range";

/** One leaf as the UI edits it: property, base operator, negation flag, value. */
export interface RichCondition {
  /** Stable UI key; never persisted. */
  key: string;
  /** Field stem, e.g. `priority` or `state_id`. */
  property: string;
  /** Base lookup without the negation wrapper, e.g. `in` or `between`. */
  operator: string;
  /** Stored as a `not` node rather than a lookup suffix. */
  negated: boolean;
  value: unknown;
}

/** Stored expression shape. Unknown keys are treated as invalid leaves. */
export type RichExpression = Record<string, unknown>;

let nextConditionKey = 0;

/** Allocate a client-only condition key. */
export function conditionKey(): string {
  nextConditionKey += 1;
  return `condition-${nextConditionKey}`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function singleLeaf(value: Record<string, unknown>): { field: string; operator: string; value: unknown } | null {
  const entries = Object.entries(value);
  if (entries.length !== 1) return null;
  const [key, leafValue] = entries[0]!;
  const split = key.lastIndexOf("__");
  if (split <= 0 || split + 2 >= key.length) return null;
  return { field: key.slice(0, split), operator: key.slice(split + 2), value: leafValue };
}

/**
 * Read one stored leaf value back into the editor's shape. Collection
 * lookups persist as one comma-separated string (the FilterSet's widget
 * reads CSV), so a multi pick reopens as its list of ids rather than as
 * one id containing commas — otherwise the next edit serializes the
 * joined string again as a single value and the pick loses every other
 * member. A date range persists as `"start,end"` and reopens as its two
 * ends so the slot can render and re-save it. A single value keeps its
 * scalar form.
 */
export function parseLeafValue(operator: string, value: unknown): unknown {
  if (typeof value !== "string" || !value.includes(",")) return value;
  if (operator === "range") {
    const ends = value.split(",").map((entry) => entry.trim());
    return ends.length === 2 && ends.every((entry) => entry !== "") ? ends : value;
  }
  if (operator !== "in") return value;
  const entries = value
    .split(",")
    .map((entry) => entry.trim())
    .filter((entry) => entry !== "");
  return entries.length === 0 ? value : entries;
}

/**
 * Read a stored lookup back as the operator the editor offers for that
 * field. A collection field's stored `__exact` came from another surface
 * (the FilterSet accepts it for one value); the builder edits it under
 * its collection lookup so a second pick keeps the same form.
 */
function parseLeafOperator(property: string, operator: string): string {
  return operator === "exact" && collectionField(property) ? "in" : operator;
}

/** A leaf's value read under the operator the editor will show. */
function parseLeafStoredValue(property: string, operator: string, value: unknown): unknown {
  return parseLeafValue(parseLeafOperator(property, operator), value);
}

function parseLeaf(node: Record<string, unknown>, key: string): RichCondition[] {
  const leaf = singleLeaf(node);
  if (leaf === null) return [invalidCondition(key, node)];
  return [
    {
      key,
      property: leaf.field,
      operator: parseLeafOperator(leaf.field, leaf.operator),
      negated: false,
      value: parseLeafStoredValue(leaf.field, leaf.operator, leaf.value),
    },
  ];
}

function invalidCondition(key: string, value: unknown): RichCondition {
  return { key, property: "", operator: "", negated: false, value };
}

/**
 * Parse a stored expression into flat conditions. `and` flattens in order;
 * `not` marks the wrapped leaf. Anything else becomes an invalid condition
 * so the row can render its error pill and offer removal rather than
 * silently discarding a stored filter.
 */
export function parseRichExpression(expression: unknown): RichCondition[] {
  if (!isRecord(expression) || Object.keys(expression).length === 0) return [];
  if ("and" in expression) {
    const children = expression["and"];
    if (!Array.isArray(children)) return [invalidCondition(conditionKey(), expression)];
    return children.flatMap((child, index) => parseExpressionChild(child, `and-${index}`));
  }
  if ("not" in expression) {
    const child = expression["not"];
    if (!isRecord(child)) return [invalidCondition(conditionKey(), expression)];
    const leaf = singleLeaf(child);
    if (leaf === null) return [invalidCondition(conditionKey(), expression)];
    return [
      {
        key: "not-0",
        property: leaf.field,
        operator: parseLeafOperator(leaf.field, leaf.operator),
        negated: true,
        value: parseLeafStoredValue(leaf.field, leaf.operator, leaf.value),
      },
    ];
  }
  const leaf = singleLeaf(expression);
  if (leaf === null) return [invalidCondition(conditionKey(), expression)];
  return [
    {
      key: "leaf-0",
      property: leaf.field,
      operator: parseLeafOperator(leaf.field, leaf.operator),
      negated: false,
      value: parseLeafStoredValue(leaf.field, leaf.operator, leaf.value),
    },
  ];
}

/** Parse one `and` child, including the `not` wrapper. */
function parseExpressionChild(child: unknown, key: string): RichCondition[] {
  if (!isRecord(child)) return [invalidCondition(key, child)];
  if ("not" in child) {
    const inner = child["not"];
    if (!isRecord(inner)) return [invalidCondition(key, child)];
    const leaf = singleLeaf(inner);
    if (leaf === null) return [invalidCondition(key, child)];
    return [
      {
        key,
        property: leaf.field,
        operator: parseLeafOperator(leaf.field, leaf.operator),
        negated: true,
        value: parseLeafStoredValue(leaf.field, leaf.operator, leaf.value),
      },
    ];
  }
  return parseLeaf(child, key);
}

/**
 * The stored lookup for a base operator and value. A collection pick keeps
 * its collection lookup for any number of picks — the FilterSet's `__in`
 * widgets accept one value as well as many, while `__exact` reads a single
 * value and rejects a joined list. A stored scalar under `exact` stays
 * scalar; the moment it grows into a list (a second pick), it moves to the
 * collection lookup so the next write is accepted.
 */
export function payloadOperator(operator: string, value: unknown): string {
  if (operator === "exact" && Array.isArray(value)) return "in";
  return operator;
}

/**
 * The list endpoint's filter leaves carry scalar strings. Multi picks
 * become one comma-separated value under `__in` (the FilterSet's
 * collection widgets parse CSV), and a date range becomes `"start,end"`
 * under `__range`.
 */
function payloadValue(value: unknown): unknown {
  if (Array.isArray(value)) {
    const entries = value.filter((entry) => typeof entry === "string" && entry !== "");
    return entries.length === 1 ? entries[0] : entries.join(",");
  }
  return value;
}

/**
 * Stems whose catalogue entry is a collection pick. Every one is a
 * collection lookup only: the FilterSet's `__in` widget takes one value
 * or many, while the scalar lookup rejects a joined list. A record
 * written by another surface can still carry `__exact`, so serialization
 * reads those back under the collection lookup.
 */
let collectionStems: ReadonlySet<string> | null = null;

function collectionField(stem: string): boolean {
  collectionStems ??= new Set(
    richFilterFields()
      .filter((field) => field.kind === "multi" || field.kind === "single")
      .map((field) => field.stem)
  );
  return collectionStems.has(stem);
}

/** Serialize one leaf; the caller wraps negation when needed. */
function serializeLeaf(condition: RichCondition): Record<string, unknown> {
  const base = condition.operator === "exact" && collectionField(condition.property) ? "in" : condition.operator;
  const operator = payloadOperator(base, condition.value);
  return { [`${condition.property}__${operator}`]: payloadValue(condition.value) };
}

/**
 * Serialize conditions back to the stored tree. Nothing becomes `{}`; one
 * positive leaf stays a bare leaf; every other case is an `and` of leaves
 * and `not` wrappers.
 */
export function serializeRichExpression(conditions: readonly RichCondition[]): RichExpression {
  const valid = conditions.filter(isConditionComplete);
  if (valid.length === 0) return {};
  if (valid.length === 1 && !valid[0]!.negated) return serializeLeaf(valid[0]!);
  if (valid.length === 1 && valid[0]!.negated) return { not: serializeLeaf(valid[0]!) };
  return {
    and: valid.map((condition) => (condition.negated ? { not: serializeLeaf(condition) } : serializeLeaf(condition))),
  };
}

/**
 * A stored expression in the shape this builder writes. The server and the
 * old app can hold a one-leaf `and` that means the same thing as the bare
 * leaf, so comparisons read both through the parser and serializer rather
 * than comparing raw trees.
 */
export function normalizeRichExpression(expression: unknown): RichExpression {
  return serializeRichExpression(parseRichExpression(expression));
}

/** Whether a condition has a property, operator and a usable value. */
export function isConditionComplete(condition: RichCondition): boolean {
  if (condition.property === "" || condition.operator === "") return false;
  if (condition.value === null || condition.value === undefined || condition.value === "") return false;
  if (Array.isArray(condition.value)) {
    return (
      condition.value.length > 0 &&
      condition.value.every((entry) => entry !== null && entry !== undefined && entry !== "")
    );
  }
  return true;
}

/** True when the stored expression has at least one parsed condition. */
export function hasRichConditions(conditions: readonly RichCondition[]): boolean {
  return conditions.length > 0;
}

/** Replace a condition's property, resetting operator/value to the field default. */
export function replaceConditionProperty(
  conditions: readonly RichCondition[],
  key: string,
  field: RichFilterField
): RichCondition[] {
  return conditions.map((condition) =>
    condition.key === key
      ? {
          key: condition.key,
          property: field.stem,
          operator: field.defaultOperator,
          negated: field.defaultNegated,
          value: null,
        }
      : condition
  );
}

/** Update one condition immutably. */
export function updateCondition(
  conditions: readonly RichCondition[],
  key: string,
  patch: Partial<Omit<RichCondition, "key">>
): RichCondition[] {
  return conditions.map((condition) => (condition.key === key ? { ...condition, ...patch } : condition));
}

/** Remove one condition by its UI key. */
export function removeCondition(conditions: readonly RichCondition[], key: string): RichCondition[] {
  return conditions.filter((condition) => condition.key !== key);
}

/** Append one condition for a field with its default operator and no value. */
export function addCondition(conditions: readonly RichCondition[], field: RichFilterField): RichCondition[] {
  return [
    ...conditions,
    {
      key: conditionKey(),
      property: field.stem,
      operator: field.defaultOperator,
      negated: field.defaultNegated,
      value: null,
    },
  ];
}

/** Field stems already used by a non-invalid condition. */
export function usedProperties(conditions: readonly RichCondition[]): Set<string> {
  return new Set(conditions.filter((condition) => condition.property !== "").map((condition) => condition.property));
}

/** One selectable filter property. */
export interface RichFilterField {
  /** Stored field stem in the expression. */
  stem: string;
  /** Visible label in the picker and condition pill. */
  label: string;
  kind: RichFilterKind;
  /** Operator used when the field is first added. */
  defaultOperator: string;
  /** Whether the default starts negated. */
  defaultNegated: boolean;
  /** Lookups offered by the operator menu, positive forms only. */
  operators: readonly string[];
  /** Whether the field offers the negated form of each lookup. */
  allowNegative: boolean;
  /** Earliest selectable day for a date kind, ISO (`YYYY-MM-DD`). */
  min?: string | undefined;
  /** Latest selectable day for a date kind, ISO (`YYYY-MM-DD`). */
  max?: string | undefined;
  /** Static values for single/multi fields, when the field needs no read. */
  staticValues?: readonly { value: string; label: string }[];
  /** Where the value list comes from; absent means the static values. */
  source?: "states" | "members" | "labels" | "projects" | "cycles" | "modules";
}

const DEFAULT_FIELDS: readonly RichFilterField[] = [
  {
    stem: "state_id",
    label: "State",
    kind: "multi",
    defaultOperator: "in",
    defaultNegated: false,
    operators: ["in"],
    allowNegative: true,
    source: "states",
  },
  {
    stem: "state_group",
    label: "State Group",
    kind: "multi",
    defaultOperator: "in",
    defaultNegated: false,
    operators: ["in"],
    allowNegative: true,
    staticValues: [
      { value: "backlog", label: "Backlog" },
      { value: "unstarted", label: "Unstarted" },
      { value: "started", label: "Started" },
      { value: "completed", label: "Completed" },
      { value: "cancelled", label: "Cancelled" },
    ],
  },
  {
    stem: "priority",
    label: "Priority",
    kind: "multi",
    defaultOperator: "in",
    defaultNegated: false,
    operators: ["in"],
    allowNegative: true,
    staticValues: (["urgent", "high", "medium", "low", "none"] as const).map((level: PriorityLevel) => ({
      value: level,
      label: level.charAt(0).toUpperCase() + level.slice(1),
    })),
  },
  {
    stem: "assignee_id",
    label: "Assignees",
    kind: "multi",
    defaultOperator: "in",
    defaultNegated: false,
    operators: ["in"],
    allowNegative: true,
    source: "members",
  },
  {
    stem: "created_by_id",
    label: "Created by",
    kind: "multi",
    defaultOperator: "in",
    defaultNegated: false,
    operators: ["in"],
    allowNegative: true,
    source: "members",
  },
  {
    stem: "mention_id",
    label: "Mentions",
    kind: "multi",
    defaultOperator: "in",
    defaultNegated: false,
    operators: ["in"],
    allowNegative: true,
    source: "members",
  },
  {
    stem: "label_id",
    label: "Label",
    kind: "multi",
    defaultOperator: "in",
    defaultNegated: false,
    operators: ["in"],
    allowNegative: true,
    source: "labels",
  },
];

/** Drop the dimensions a project with those features switched off must not offer. */
export function hideRichFilterFields(
  fields: readonly RichFilterField[],
  input: { hideCycle?: boolean; hideModule?: boolean }
): RichFilterField[] {
  return fields.filter((field) => {
    if (field.stem === "cycle_id" && (input.hideCycle ?? false)) return false;
    if (field.stem === "module_id" && (input.hideModule ?? false)) return false;
    return true;
  });
}

/**
 * The field catalogue for one project surface: the core choice fields
 * plus the value-input fields this slice edits (ISS-100–103). Cycle and
 * module disappear when their project features are off.
 */
export function richFilterFields(input?: { hideCycle?: boolean; hideModule?: boolean }): RichFilterField[] {
  const fields = withValueInputFields(DEFAULT_FIELDS);
  return hideRichFilterFields(fields, input ?? {});
}

/** Find a field by its stored stem. */
export function findField(fields: readonly RichFilterField[], stem: string): RichFilterField | null {
  return fields.find((field) => field.stem === stem) ?? null;
}

/** A condition whose property no longer resolves. */
export function isUnresolvableCondition(condition: RichCondition, fields: readonly RichFilterField[]): boolean {
  return condition.property === "" || findField(fields, condition.property) === null;
}

/** Human label for a base operator. */
export const OPERATOR_LABELS: Record<string, string> = {
  in: "is",
  exact: "is",
  range: "between",
};

/** One operator option; `negated` flips to the "is not" family. */
export interface OperatorOption {
  base: string;
  negated: boolean;
  label: string;
}

/**
 * Operator options for a field. Fields that allow negation render the plain
 * and negated form of each lookup (ISS-099); a field with a single form
 * keeps its operator control locked.
 */
export function operatorOptions(field: RichFilterField): OperatorOption[] {
  const options: OperatorOption[] = [];
  for (const base of field.operators) {
    const label = OPERATOR_LABELS[base] ?? base;
    options.push({ base, negated: false, label });
    if (field.allowNegative) options.push({ base, negated: true, label: negatedLabel(base) });
  }
  return options;
}

/** Whether the operator control must render disabled. */
export function isOperatorLocked(field: RichFilterField): boolean {
  return operatorOptions(field).length <= 1;
}

/** Label shown on the operator control. */
export function operatorLabel(operator: string, negated: boolean): string {
  const label = OPERATOR_LABELS[operator] ?? operator;
  return negated ? negatedLabel(operator) : label;
}

function negatedLabel(operator: string): string {
  return operator === "in" || operator === "exact" ? "is not" : `not ${OPERATOR_LABELS[operator] ?? operator}`;
}

/** Date-range value shape used by the value slot boundary. */
export interface RichDateRange {
  start: string | null;
  end: string | null;
}

/** Whether a value is complete enough to commit for the field kind. */
export function isValueComplete(kind: RichFilterKind, value: unknown): boolean {
  if (kind === "date_range") {
    if (!Array.isArray(value) || value.length !== 2) return false;
    return value.every((entry) => typeof entry === "string" && entry.length > 0);
  }
  if (kind === "multi") return Array.isArray(value) && value.length > 0;
  if (kind === "date") return typeof value === "string" && value.length > 0;
  return value !== null && value !== undefined && value !== "";
}

/**
 * Completeness for one operator's value shape. A date field's `exact`
 * lookup holds one day while its `range` lookup holds both ends, so the
 * kind alone cannot decide whether the editor is finished.
 */
export function isValueCompleteForOperator(field: RichFilterField, operator: string, value: unknown): boolean {
  if (operator === "range") return isValueComplete("date_range", value);
  if (operator === "exact" && field.kind === "date_range") return isValueComplete("date", value);
  // A stored collection filter may arrive under `exact` with one scalar
  // value (the filter set accepts both forms); it is still a complete
  // pick, so the slot neither warns nor reopens on load.
  if (field.kind === "multi") {
    if (typeof value === "string") return value.length > 0;
    return isValueComplete("multi", value);
  }
  return isValueComplete(field.kind, value);
}

/** Human label for one stored value, or null when the entity no longer resolves. */
export function valueLabel(field: RichFilterField, value: unknown): string | null {
  if (field.kind === "date_range") {
    return Array.isArray(value) && value.length === 2 && typeof value[0] === "string" && typeof value[1] === "string"
      ? `${value[0]} → ${value[1]}`
      : null;
  }
  const raw = Array.isArray(value) ? value : [value];
  const labels: string[] = [];
  for (const entry of raw) {
    if (typeof entry !== "string") return null;
    const staticValue = field.staticValues?.find((option) => option.value === entry);
    if (staticValue !== undefined) {
      labels.push(staticValue.label);
    } else if (field.kind === "date") {
      labels.push(entry);
    } else {
      return null;
    }
  }
  return labels.length === 0 ? null : labels.join(", ");
}

/** Build a catalogue entry with the common defaults spelled once. */
function catalogField(input: {
  stem: string;
  label: string;
  kind: RichFilterKind;
  operators?: readonly string[];
  allowNegative?: boolean;
  source?: RichFilterField["source"];
  min?: string;
  max?: string;
}): RichFilterField {
  const operators = input.operators ?? ["in"];
  return {
    stem: input.stem,
    label: input.label,
    kind: input.kind,
    defaultOperator: operators[0]!,
    defaultNegated: false,
    operators,
    allowNegative: input.allowNegative ?? true,
    ...(input.source === undefined ? {} : { source: input.source }),
    ...(input.min === undefined ? {} : { min: input.min }),
    ...(input.max === undefined ? {} : { max: input.max }),
  };
}

/**
 * Value-input fields (parity/specs/issues.md §9, ISS-100–103). Each entry
 * carries the kind its value slot renders: searchable multi picks over the
 * project (assignee, creator, mentions, labels, project, cycle, module),
 * one bounded date (target date, no clear control) and a bounded date range
 * (start date, which also offers the plain is lookup the oracle drives).
 * Stems are the filter set's field names; entity-backed fields resolve
 * their labels from the shared pickers, so reads stay lazy on open.
 */
export function valueInputFields(): RichFilterField[] {
  return [
    catalogField({ stem: "project_id", label: "Project", kind: "multi", source: "projects" }),
    catalogField({ stem: "cycle_id", label: "Cycle", kind: "multi", source: "cycles" }),
    catalogField({ stem: "module_id", label: "Module", kind: "multi", source: "modules" }),
    catalogField({ stem: "target_date", label: "Target date", kind: "date", operators: ["exact"] }),
    catalogField({ stem: "start_date", label: "Start date", kind: "date_range", operators: ["exact", "range"] }),
  ];
}

/** Merge the value-input fields into the base catalogue without duplicates. */
export function withValueInputFields(fields: readonly RichFilterField[]): RichFilterField[] {
  const known = new Set(fields.map((field) => field.stem));
  return [...fields, ...valueInputFields().filter((field) => !known.has(field.stem))];
}
