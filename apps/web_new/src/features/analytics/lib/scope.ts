// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics project scope (parity/specs/analytics.md §4, ANL-004).
// Pure: the selectable-project rule, the request scope and the picker
// summary wording. The address parsing lives in search.ts, which the tab
// route imports directly so this module's users stay off its chunk
// (NEWFRONT-871). Nothing here touches React, the router, or the network.

import type { Project } from "@pidash/api-client";

import { normalizeAnalyticsScope } from "./search.js";

export { analyticsScopeSearch, normalizeAnalyticsScope, parseAnalyticsSearch, type AnalyticsSearch } from "./search.js";

/** Project fields the scope rules read. */
export type ScopeProject = Pick<Project, "id" | "name" | "member_role" | "archived_at">;

/**
 * Projects the analytics screen may scope to: joined by the caller
 * (a workspace role on the row) and not archived. The same set
 * drives the no-project empty view, the picker options, and the
 * active-projects join. Generic over the row so callers keep their
 * wider fields (identity, marks).
 */
export function selectAnalyticsProjects<P extends ScopeProject>(projects: readonly P[]): P[] {
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
