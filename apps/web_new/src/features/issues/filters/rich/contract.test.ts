// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Filter-key contract (parity/specs/issues.md §9, ISS-100–103). The list
// endpoint validates every leaf name against its IssueFilterSet and
// rejects anything it does not declare, so this suite mirrors that
// allowlist and proves the builder can only emit accepted names, with the
// value shape each lookup's widget reads.
import { describe, expect, it } from "vitest";

import {
  findField,
  parseRichExpression as parseRich,
  richFilterFields,
  serializeRichExpression,
  type RichFilterField,
} from "./expression.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";

/**
 * Mirrors `apps/api/pi_dash/utils/filters/filterset.py` (IssueFilterSet):
 * the ID/static filters accept the bare name (and `__exact`, which
 * BaseFilterSet derives) plus `__in`; priority adds its own `__in`; dates
 * accept `__exact` and `__range`. Keep in step with that file.
 */
const ID_FILTERS = [
  "assignee_id",
  "mention_id",
  "label_id",
  "created_by_id",
  "state_group",
  "state_id",
  "project_id",
  "cycle_id",
  "module_id",
  "subscriber_id",
];
const STATIC_FILTERS = ["priority"];
const DATE_FILTERS = ["start_date", "target_date", "created_at", "updated_at"];

function acceptedKeys(): Set<string> {
  const keys = new Set<string>();
  for (const field of [...ID_FILTERS, ...STATIC_FILTERS]) {
    keys.add(field);
    keys.add(`${field}__exact`);
    keys.add(`${field}__in`);
  }
  for (const field of DATE_FILTERS) {
    keys.add(`${field}__exact`);
    keys.add(`${field}__range`);
  }
  return keys;
}

function leaves(expression: Record<string, unknown>): string[] {
  const found: string[] = [];
  const walk = (node: unknown): void => {
    if (typeof node !== "object" || node === null || Array.isArray(node)) return;
    for (const [key, value] of Object.entries(node)) {
      if (key === "and" || key === "or") {
        for (const child of value as unknown[]) walk(child);
      } else if (key === "not") {
        walk(value);
      } else {
        found.push(key);
      }
    }
  };
  walk(expression);
  return found;
}

const ACCEPTED = acceptedKeys();

function expectsAccepted(expression: Record<string, unknown>): void {
  const keys = leaves(expression);
  expect(keys.length).toBeGreaterThan(0);
  for (const key of keys) expect(ACCEPTED.has(key), `server rejects ${key}`).toBe(true);
}

describe("the catalogue emits only filter names IssueFilterSet declares", () => {
  it("keeps one value and several under the collection lookup", () => {
    // `UUIDInFilter` accepts a lone id; `assignee_id__exact` is a scalar
    // lookup that rejects the CSV form, so a pick never stores under it.
    const single = serializeRichExpression([
      { key: "a", property: "assignee_id", operator: "in", negated: false, value: [UUID] },
    ]);
    expect(single).toEqual({ assignee_id__in: UUID });
    expectsAccepted(single);
    const exact = serializeRichExpression([
      { key: "a", property: "assignee_id", operator: "exact", negated: false, value: [UUID] },
    ]);
    expect(exact).toEqual({ assignee_id__in: UUID });
    expectsAccepted(exact);
    const several = serializeRichExpression([
      { key: "a", property: "assignee_id", operator: "in", negated: false, value: [UUID, `${UUID}2`] },
    ]);
    expect(several).toEqual({ assignee_id__in: `${UUID},${UUID}2` });
    expectsAccepted(several);
  });

  it("accepts every catalogue field in both plain and negated form", () => {
    for (const field of richFilterFields()) {
      for (const operator of field.operators) {
        const value = sampleValue(field, operator);
        for (const negated of [false, true]) {
          expectsAccepted(serializeRichExpression([{ key: "a", property: field.stem, operator, negated, value }]));
        }
      }
    }
  });
});

function sampleValue(field: RichFilterField, operator: string): unknown {
  if (operator === "range") return ["2026-01-01", "2026-01-31"];
  if (field.kind === "date_range") return "2026-01-01";
  if (field.kind === "date") return "2026-01-01";
  return field.staticValues?.[0]?.value ?? [UUID];
}

describe("the contract mirror itself", () => {
  it("matches the fields the catalogue offers", () => {
    for (const field of richFilterFields()) {
      expect(ACCEPTED.has(`${field.stem}__${field.defaultOperator}`), field.stem).toBe(true);
    }
  });

  it("rejects a key the backend does not declare", () => {
    expect(leaves({ assignee__in: "a" }).every((key) => ACCEPTED.has(key))).toBe(false);
    expect(leaves({ priority__between: "a,b" }).every((key) => ACCEPTED.has(key))).toBe(false);
  });

  it("writes a stored one-pick exact back under the collection lookup", () => {
    const leavesUnder = (stored: Record<string, unknown>): string[] =>
      leaves(serializeRichExpression(parseRich(stored)));
    expect(leavesUnder({ state_id__exact: UUID })).toEqual(["state_id__in"]);
    expect(leavesUnder({ and: [{ priority__in: "urgent" }, { not: { state_id__exact: UUID } }] })).toEqual([
      "priority__in",
      "state_id__in",
    ]);
    for (const key of [...leavesUnder({ state_id__exact: UUID }), ...leavesUnder({ priority__exact: "urgent" })]) {
      expect(ACCEPTED.has(key), `server rejects ${key}`).toBe(true);
    }
  });

  it("still finds the value fields the issues surface offers", () => {
    expect(findField(richFilterFields(), "label_id")?.kind).toBe("multi");
  });
});
