// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics project scope (parity/specs/analytics.md §4, ANL-004).
// Pure: scope normalization, the selectable-project rule, search
// parsing/serialization for the project_ids address parameter, and
// the picker summary wording. Components and routes build on these;
// nothing here touches React, the router, or the network.

import type { Project } from "@pidash/api-client";

/** Project fields the scope rules read. */
export type ScopeProject = Pick<Project, "id" | "name" | "member_role" | "archived_at">;

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

/**
 * Projects the analytics screen may scope to: joined by the caller
 * (a workspace role on the row) and not archived. The same set
 * drives the no-project empty view and the picker options.
 */
export function selectAnalyticsProjects(projects: readonly ScopeProject[]): ScopeProject[] {
  return projects.filter((project) => project.member_role !== null && project.archived_at === null);
}

/**
 * The request scope for one address selection: normalized, then
 * narrowed to ids the caller can still resolve. Unresolvable ids
 * stay in the address but leave labels and requests (spec §4).
 */
export function resolveAnalyticsScope(ids: readonly string[], projects: readonly ScopeProject[]): string[] {
  const selectable = new Set(selectAnalyticsProjects(projects).map((project) => project.id));
  return normalizeAnalyticsScope(ids).filter((id) => selectable.has(id));
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

/** Picker trigger text when nothing is selected. */
export const ALL_PROJECTS_LABEL = "All projects";

/** Picker trigger text once four or more projects are selected (spec §4: a fixed literal). */
export const MANY_PROJECTS_LABEL = "3+ projects";

/**
 * The picker trigger summary for resolved selection names in
 * selection order: the all-projects label, one name, a joined name
 * list, or the fixed long-selection literal.
 */
export function selectPickerSummary(names: readonly string[]): string {
  if (names.length === 0) return ALL_PROJECTS_LABEL;
  if (names.length >= 4) return MANY_PROJECTS_LABEL;
  return names.join(", ");
}
