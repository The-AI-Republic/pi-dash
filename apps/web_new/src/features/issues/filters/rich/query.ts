// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Rich-filter query bridge (parity/specs/issues.md §9, ISS-095–099,
// ISS-092). The list endpoint reads the complex filter JSON from its
// `filters` query parameter; this module turns the stored expression into
// that parameter and leaves it absent when no condition applies.
//
// Start and target dates are the exception. A relative window only means
// something to the endpoint's bare `start_date` / `target_date` parameters,
// which resolve it against today, so a date condition travels there in the
// legacy date grammar (entries joined with commas). The JSON tree keeps
// every other leaf, so nothing is applied twice.

import type { IssueListQuery } from "@pidash/api-client";

import { customEndEntry, customStartEntry, dateEntries } from "./legacyDimensions.js";
import type { RichExpression } from "./expression.js";

/** Date fields forwarded as bare list parameters rather than JSON leaves. */
export const LEGACY_DATE_FIELDS = ["start_date", "target_date"] as const;

/** Stringify an expression for the list's `filters` parameter. */
export function richFiltersParam(expression: RichExpression): string | null {
  return Object.keys(expression).length === 0 ? null : JSON.stringify(expression);
}

/** An entry that is a bare ISO day with no direction yet. */
const BARE_DAY = /^\d{4}-\d{2}-\d{2}$/;

/**
 * The stored date entries for one condition value, in the server's legacy
 * grammar. Presets already carry it, and this slice stores custom days with
 * their direction. A bare `"start,end"` pair (how a stored range used to be
 * written) takes lower and upper bounds by position so the condition keeps
 * both ends.
 */
export function legacyDateEntries(value: unknown): string[] {
  let bare = 0;
  return dateEntries(value).map((entry) => {
    if (!BARE_DAY.test(entry)) return entry;
    const directed = bare === 0 ? customStartEntry(entry) : customEndEntry(entry);
    bare += 1;
    return directed;
  });
}

/** Whether a leaf is one the date bridge takes out of the JSON tree. */
function isLegacyDateKey(key: string): boolean {
  const stem = key.split("__")[0];
  return LEGACY_DATE_FIELDS.includes(stem as (typeof LEGACY_DATE_FIELDS)[number]);
}

/**
 * Remove date leaves from a stored tree, collecting their values. A tree
 * left with no conditions reads as empty; an `and` with one remaining
 * child collapses to that child so the payload stays in the shape the
 * builder writes.
 */
export function splitDateConditions(expression: RichExpression): {
  expression: RichExpression;
  dates: Record<string, string[]>;
} {
  const dates: Record<string, string[]> = {};
  const collect = (field: string, value: unknown): void => {
    for (const entry of legacyDateEntries(value)) {
      const bucket = (dates[field] ??= []);
      if (!bucket.includes(entry)) bucket.push(entry);
    }
  };
  const prune = (node: unknown): unknown => {
    if (typeof node !== "object" || node === null || Array.isArray(node)) return node;
    const record = node as Record<string, unknown>;
    if ("and" in record && Array.isArray(record["and"])) {
      const children = record["and"].map(prune).filter((child) => child !== null);
      if (children.length === 0) return null;
      if (children.length === 1) return children[0];
      return { and: children };
    }
    if ("not" in record) {
      const child = prune(record["not"]);
      return child === null ? null : { not: child };
    }
    const entries = Object.entries(record);
    if (entries.length !== 1) return node;
    const [key, value] = entries[0]!;
    if (!isLegacyDateKey(key)) return node;
    collect(key.split("__")[0]!, value);
    return null;
  };
  const pruned = prune(expression);
  return {
    expression: pruned === null ? {} : (pruned as RichExpression),
    dates,
  };
}

/** Add the stored expression and its date entries to an issue-list query. */
export function withRichFilters(query: IssueListQuery, expression: RichExpression): IssueListQuery {
  const split = splitDateConditions(expression);
  const filters = richFiltersParam(split.expression);
  const dates = Object.entries(split.dates).filter(([, entries]) => entries.length > 0);
  if (filters === null && dates.length === 0) return query;
  const next: IssueListQuery = { ...query };
  if (filters !== null) next.filters = { ...query.filters, filters };
  if (dates.length > 0) {
    next.filters = { ...next.filters };
    for (const [field, entries] of dates) next.filters[field] = entries.join(",");
  }
  return next;
}
