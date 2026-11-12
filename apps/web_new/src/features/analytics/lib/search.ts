// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics address search (parity/specs/analytics.md §4, ANL-004). Pure:
// the project_ids parameter as the tab route reads and writes it, plus the
// normalization both the address and the request scope share. Kept apart
// from the picker and request selection rules so the route module (eager
// in the initial chain) carries only these helpers (NEWFRONT-871, size
// gate).

/** Query key + request scope: blank ids dropped, duplicates removed, order kept. */
export function normalizeAnalyticsScope(ids: readonly string[]): string[] {
  const seen = new Set<string>();
  const normalized: string[] = [];
  for (const id of ids) {
    const trimmed = id.trim();
    if (trimmed === "" || seen.has(trimmed)) continue;
    seen.add(trimmed);
    normalized.push(trimmed);
  }
  return normalized;
}

/** Validated analytics search: the project id list, when present. */
export interface AnalyticsSearch {
  project_ids?: string[] | undefined;
}

/**
 * Validate the tab addresses' search. Accepts one comma-separated
 * value or a repeated parameter; anything else is ignored so a
 * hand-edited address degrades to the full workspace.
 */
export function parseAnalyticsSearch(search: Record<string, unknown>): AnalyticsSearch {
  const raw = search["project_ids"];
  const ids =
    typeof raw === "string"
      ? raw.split(",")
      : Array.isArray(raw)
        ? raw.filter((entry): entry is string => typeof entry === "string").flatMap((entry) => entry.split(","))
        : [];
  const normalized = normalizeAnalyticsScope(ids);
  return normalized.length === 0 ? {} : { project_ids: normalized };
}

/** The search update a picker change navigates to; empty clears the parameter. */
export function analyticsScopeSearch(ids: readonly string[]): AnalyticsSearch {
  const normalized = normalizeAnalyticsScope(ids);
  return normalized.length === 0 ? {} : { project_ids: normalized };
}
