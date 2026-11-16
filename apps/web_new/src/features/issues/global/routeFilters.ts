// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// URL route filters for the workspace all-issues view
// (parity/specs/issues.md §18, ISS-223). The view reads every query
// string parameter into a route-filter record when it loads, so a deep
// link keeps its parameters on the address bar. In this build no list
// consumer reads the record yet: the oracle pins the unfiltered result
// for `?priority=urgent&order_by=-created_at`. The parser therefore
// never throws on unknown or repeated keys.

/** One parsed parameter: the last value a repeated key carried. */
export type RouteFilters = Record<string, string>;

/**
 * Read the URL's query parameters into route filters. Repeated keys
 * keep their last value; an empty value stays an empty string so a
 * later consumer can tell "present but blank" from "absent".
 */
export function parseRouteFilters(search: Record<string, unknown>): RouteFilters {
  const filters: RouteFilters = {};
  for (const [key, value] of Object.entries(search)) {
    if (value === undefined || value === null) continue;
    if (Array.isArray(value)) {
      const last = value[value.length - 1];
      if (typeof last === "string" || typeof last === "number" || typeof last === "boolean") {
        filters[key] = String(last);
      }
      continue;
    }
    if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
      filters[key] = String(value);
    }
  }
  return filters;
}

/** True when a route-filter record carries at least one parameter. */
export function hasRouteFilters(filters: RouteFilters): boolean {
  return Object.keys(filters).length > 0;
}
