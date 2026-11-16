// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issues-bearing entities (parity/specs/issues.md §1, ISS-001/ISS-002).
// Each entity carries its own layout root and its own user-properties
// record. Reads live in the eager prefsQuery leaf (the route prefetches
// them); this module holds the write side plus the shared types, loaded
// with the lazy layout code.

import {
  updateCycleUserProperties,
  updateEpicUserProperties,
  updateModuleUserProperties,
  updateProjectDisplayPreferences,
  updateWorkspaceDisplayPreferences,
  type ApiClient,
  type PrefsPatch,
} from "@pidash/api-client";

/** An issues-bearing entity: project, cycle, module, epics, or workspace. */
export type IssuesEntity =
  | { scope: "project"; projectId: string }
  | { scope: "cycle"; projectId: string; cycleId: string }
  | { scope: "module"; projectId: string; moduleId: string }
  | { scope: "epics"; projectId: string }
  | { scope: "workspace" };

/** The four preference keys every user-properties record carries. */
export interface PrefsRecord {
  filters: Record<string, unknown>;
  display_filters: Record<string, unknown>;
  display_properties: Record<string, unknown>;
  rich_filters: Record<string, unknown>;
}

/**
 * Overlay a full record's `keys` onto the cached prefs record
 * (NEWFRONT-1013). Success and rollback writes go through here so an
 * interleaved mutation's keys survive: each writer touches only the keys
 * it sent, never the whole record, because the response's other keys are
 * stale whenever two prefs writes overlap. An empty cache falls back to
 * the source record.
 */
export function mergePrefsResponse(
  current: PrefsRecord | undefined,
  source: PrefsRecord,
  keys: readonly (keyof PrefsRecord)[]
): PrefsRecord {
  if (current === undefined) return source;
  const merged: PrefsRecord = { ...current };
  for (const key of keys) merged[key] = source[key];
  return merged;
}

/**
 * Overlay an optimistic patch onto the cached prefs record
 * (NEWFRONT-1013). Updater form: two mutations that both read the cache
 * before either optimistic write lands still converge instead of the
 * second write clobbering the first. An empty cache falls back to the
 * record the patch was built from.
 */
export function applyPrefsPatch(
  current: PrefsRecord | undefined,
  fallback: PrefsRecord,
  patch: PrefsPatch
): PrefsRecord {
  const base = current ?? fallback;
  return {
    filters: patch.filters ?? base.filters,
    display_filters: patch.display_filters ?? base.display_filters,
    display_properties: patch.display_properties ?? base.display_properties,
    rich_filters: patch.rich_filters ?? base.rich_filters,
  };
}

/** Key parts beneath `issueKeys.lists` for one entity's lists. */
export function entityListsKeyPart(entity: IssuesEntity): string[] {
  switch (entity.scope) {
    case "project":
      // The flat list already nests here (issueKeys.list).
      return [entity.projectId];
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

/** Patch one entity's display keys through its contract. */
export function writeEntityPrefs(
  client: ApiClient,
  workspaceSlug: string,
  entity: IssuesEntity,
  patch: PrefsPatch
): Promise<PrefsRecord> {
  switch (entity.scope) {
    case "project":
      return updateProjectDisplayPreferences(client, workspaceSlug, entity.projectId, patch);
    case "cycle":
      return updateCycleUserProperties(client, workspaceSlug, entity.projectId, entity.cycleId, patch);
    case "module":
      return updateModuleUserProperties(client, workspaceSlug, entity.projectId, entity.moduleId, patch);
    case "epics":
      return updateEpicUserProperties(client, workspaceSlug, entity.projectId, patch);
    case "workspace":
      return updateWorkspaceDisplayPreferences(client, workspaceSlug, patch);
  }
}
