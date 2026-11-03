// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Live-list name matching (parity/specs/cycles.md §3, CYC-005). The
// query narrows the already loaded rows by cycle name only,
// case-insensitive substring; it never issues a request, and an empty
// query keeps every row.

/** Whether one cycle name matches the typed query. */
export function matchesCycleQuery(name: string, query: string): boolean {
  if (query === "") return true;
  return name.toLowerCase().includes(query.toLowerCase());
}

/** Narrow loaded rows to the query, keeping their order. */
export function filterCyclesByQuery<T extends { name: string }>(cycles: readonly T[], query: string): T[] {
  if (query === "") return [...cycles];
  return cycles.filter((cycle) => matchesCycleQuery(cycle.name, query));
}
