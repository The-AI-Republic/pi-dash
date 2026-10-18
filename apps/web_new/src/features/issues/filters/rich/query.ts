// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Rich-filter query bridge (parity/specs/issues.md §9, ISS-095–099).
// The list endpoint reads the complex filter JSON from its `filters`
// query parameter; this module turns the stored expression into that
// parameter and leaves it absent when no condition applies.

import type { IssueListQuery } from "@pidash/api-client";

import type { RichExpression } from "./expression.js";

/** Stringify an expression for the list's `filters` parameter. */
export function richFiltersParam(expression: RichExpression): string | null {
  return Object.keys(expression).length === 0 ? null : JSON.stringify(expression);
}

/** Add the stored expression to an issue-list query. */
export function withRichFilters(query: IssueListQuery, expression: RichExpression): IssueListQuery {
  const filters = richFiltersParam(expression);
  if (filters === null) return query;
  return { ...query, filters: { ...query.filters, filters } };
}
