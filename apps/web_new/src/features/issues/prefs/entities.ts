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
