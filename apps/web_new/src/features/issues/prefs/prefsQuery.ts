// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Preference reads (parity/specs/issues.md §1, ISS-002). EAGER LEAF: the
// issues route prefetches these in beforeLoad, so this module must stay
// small and import nothing the lazy layout code owns — a module shared
// between a route's eager closure and its lazy chunks collapses the
// TanStack component split and trips the size gate (NEWFRONT-287).
// Writes, parsing and hooks live beside it, lazy-only.

import { queryOptions } from "@tanstack/react-query";
import {
  getCycleUserProperties,
  getEpicUserProperties,
  getModuleUserProperties,
  getProjectUserProperties,
  getWorkspaceUserProperties,
  type ApiClient,
} from "@pidash/api-client";

import { issueKeys } from "../api/keys.js";
import type { IssuesEntity, PrefsRecord } from "./entities.js";

/**
 * The sort order inside one display_filters object (ISS-002). Manual by
 * default; the issues list sends it as `order_by` so the timeline shows
 * the same order the Display panel names. Saved views read their own
 * stored filters, entity lists their user-properties record.
 */
export function orderByFromDisplayFilters(displayFilters: Record<string, unknown>): string {
  const value = displayFilters["order_by"];
  return typeof value === "string" && value.length > 0 ? value : "sort_order";
}

/** The stored sort order of one user-properties record (ISS-002). */
export function orderByOf(record: PrefsRecord): string {
  return orderByFromDisplayFilters(record.display_filters);
}

/** Key parts beneath `issueKeys.prefs` for one entity. */
export function entityPrefsKeyPart(entity: IssuesEntity): string[] {
  switch (entity.scope) {
    case "project":
      return ["project", entity.projectId];
    case "cycle":
      return ["cycle", entity.projectId, entity.cycleId];
    case "module":
      return ["module", entity.projectId, entity.moduleId];
    case "epics":
      return ["epics", entity.projectId];
    case "workspace":
      return ["workspace"];
  }
}

/** Read one entity's user-properties record through its contract. */
export function readEntityPrefs(client: ApiClient, workspaceSlug: string, entity: IssuesEntity): Promise<PrefsRecord> {
  switch (entity.scope) {
    case "project":
      return getProjectUserProperties(client, workspaceSlug, entity.projectId);
    case "cycle":
      return getCycleUserProperties(client, workspaceSlug, entity.projectId, entity.cycleId);
    case "module":
      return getModuleUserProperties(client, workspaceSlug, entity.projectId, entity.moduleId);
    case "epics":
      return getEpicUserProperties(client, workspaceSlug, entity.projectId);
    case "workspace":
      return getWorkspaceUserProperties(client, workspaceSlug);
  }
}

/** One entity's preference record: fetched once, never on window focus. */
export function prefsQueryOptions(client: ApiClient, workspaceSlug: string, entity: IssuesEntity) {
  return queryOptions({
    queryKey: issueKeys.prefs(workspaceSlug, ...entityPrefsKeyPart(entity)),
    queryFn: () => readEntityPrefs(client, workspaceSlug, entity),
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}
