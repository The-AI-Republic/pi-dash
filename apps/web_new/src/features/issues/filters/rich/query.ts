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
// which resolve it against today, so a preset travels there in the legacy
// date grammar (entries joined with commas). Custom days keep their JSON
// leaf — an exact day stays `__exact`, a two-end range stays `__range` — so
// they are neither widened to an open bound nor applied twice. A date leaf
// that a stored record keeps under a negation stays where it is: the bare
// parameters have no negated form, and silently reading one as its opposite
// would flip the condition.

import type { IssueListQuery } from "@pidash/api-client";

import { customDateMode, customRange, dateValueParts } from "./legacyDimensions.js";
import type { RichExpression } from "./expression.js";

/** Date fields forwarded as bare list parameters rather than JSON leaves. */
export const LEGACY_DATE_FIELDS = ["start_date", "target_date"] as const;

/**
 * The entries of a date value the bare parameter can carry truthfully.
 * Every one of them is a positive bound there (`after` reads `gte`,
 * `before` reads `lte`), so a relative window stored as "before" — the
 * negated direction the old list offered — is never forwarded. A custom
 * entry carries a direction only in the After and Before modes; an exact
 * day or a two-end range stays a JSON leaf, where the filter set reads it
 * exactly.
 */
function forwardableDateEntries(value: unknown): string[] {
  const mode = customDateMode(value);
  const parts = dateValueParts(value);
  const open = mode === "after" || mode === "before" ? parts.custom : [];
  // The offered presets are forward windows (`...;after;fromnow`); the bare
  // parameter reads an entry without a direction as a positive bound too.
  // A preset stored as a "before" window stays in the JSON tree.
  const presets = parts.presets.filter((entry) => entry.split(";")[1] !== "before");
  return [...presets, ...open];
}

/** Stringify an expression for the list's `filters` parameter. */
export function richFiltersParam(expression: RichExpression): string | null {
  return Object.keys(expression).length === 0 ? null : JSON.stringify(expression);
}

/** Whether a leaf is one the date bridge takes out of the JSON tree. */
function isLegacyDateKey(key: string): boolean {
  const stem = key.split("__")[0];
  return LEGACY_DATE_FIELDS.includes(stem as (typeof LEGACY_DATE_FIELDS)[number]);
}

/**
 * The JSON leaf a date value keeps when part of it leaves for the bare
 * parameters. A complete custom pair stays a `__range`; one exact day stays
 * a `__exact`; nothing custom leaves nothing behind. One-sided custom
 * bounds never reach this point: they travel with the bare parameter, which
 * is the only place the endpoint reads a direction from.
 */
function customDateLeaf(stem: string, value: unknown): RichExpression | null {
  const mode = customDateMode(value);
  // An open bound has no exact/range equivalent: it travels as its bare
  // entry, so nothing of it stays behind.
  if (mode === "after" || mode === "before") return null;
  const { start, end } = customRange(value);
  // `customDateMode` reads any two custom entries as a pair, so the range
  // mode always has both ends here.
  if (mode === "range" && start !== null && end !== null) return { [`${stem}__range`]: `${start},${end}` };
  const day = start ?? end;
  return day === null ? null : { [`${stem}__exact`]: day };
}

/**
 * Remove date leaves from a stored tree, collecting their values. A tree
 * left with no conditions reads as empty; an `and` with one remaining
 * child collapses to that child so the payload stays in the shape the
 * builder writes. A leaf moves out of the tree only when it holds a
 * positive relative window or an open custom bound; a custom day or pair
 * stays in the tree beside whatever the parameter carries (ISS-092).
 */
export function splitDateConditions(expression: RichExpression): {
  expression: RichExpression;
  dates: Record<string, string[]>;
} {
  const dates: Record<string, string[]> = {};
  const prune = (node: unknown): unknown => {
    if (typeof node !== "object" || node === null || Array.isArray(node)) return node;
    const record = node as Record<string, unknown>;
    if ("and" in record && Array.isArray(record["and"])) {
      const children = record["and"].map(prune).filter((child) => child !== null);
      if (children.length === 0) return null;
      if (children.length === 1) return children[0];
      return { and: children };
    }
    // A negated date leaf is not rewritten: the bare parameters only express
    // positive bounds, so the tree keeps the negation it was given.
    if ("not" in record) return record;
    const leaves = Object.entries(record);
    if (leaves.length !== 1) return node;
    const [key, value] = leaves[0]!;
    if (!isLegacyDateKey(key)) return node;
    const stem = key.split("__")[0]!;
    const forwarded = forwardableDateEntries(value);
    if (forwarded.length === 0) return node;
    const bucket = (dates[stem] ??= []);
    for (const entry of forwarded) if (!bucket.includes(entry)) bucket.push(entry);
    return customDateLeaf(stem, value);
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
