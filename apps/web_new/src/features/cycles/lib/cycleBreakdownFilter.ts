// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Breakdown filter toggle (parity/specs/cycles.md §14–15, CYC-039 and
// CYC-041). Hero and sidebar entries share this merge: clicking an
// entry toggles its value inside the matching rich-filter dimension,
// drops the dimension when emptied, and leaves every other condition
// — including negated and foreign ones — untouched, so entries combine
// across visits instead of replacing each other.

import type { RichCondition } from "../../issues/index.js";

/** The rich-filter dimensions breakdown entries drive. */
export type BreakdownDimension = "state_group" | "assignee_id" | "label_id" | "priority";

let nextHeroFilterKey = 0;

/** Fresh stable key for an appended condition. */
export function heroFilterKey(): string {
  nextHeroFilterKey += 1;
  return `hero-filter-${nextHeroFilterKey}`;
}

function conditionValues(condition: RichCondition): string[] {
  if (Array.isArray(condition.value)) {
    return condition.value.filter((entry): entry is string => typeof entry === "string");
  }
  return typeof condition.value === "string" ? [condition.value] : [];
}

function isDimension(condition: RichCondition, dimension: BreakdownDimension): boolean {
  if (condition.property !== dimension || condition.negated) return false;
  // A stored `exact` came from another surface; the collection lookup
  // edits it under `in` so a second pick keeps the same form.
  return condition.operator === "in" || condition.operator === "exact";
}

/**
 * Toggle one value inside one dimension: absent appends (a new
 * dimension or a second value), present removes, and an emptied
 * dimension drops out. Pure; the caller persists the result.
 */
export function toggleBreakdownFilter(
  conditions: readonly RichCondition[],
  dimension: BreakdownDimension,
  value: string
): RichCondition[] {
  const at = conditions.findIndex((condition) => isDimension(condition, dimension));
  if (at === -1) {
    return [
      ...conditions,
      { key: heroFilterKey(), property: dimension, operator: "in", negated: false, value: [value] },
    ];
  }
  const current = conditions[at]!;
  const values = conditionValues(current);
  if (values.includes(value)) {
    const next = values.filter((entry) => entry !== value);
    if (next.length === 0) return conditions.filter((_, index) => index !== at);
    return conditions.map((condition, index) =>
      index === at ? { ...condition, operator: "in", value: next } : condition
    );
  }
  return conditions.map((condition, index) =>
    index === at ? { ...condition, operator: "in", value: [...values, value] } : condition
  );
}
