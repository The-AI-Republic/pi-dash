// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module query keys (parity/specs/modules.md §13, §16.9). Every key starts
// with the workspace scope so a workspace switch or sign-out clears one
// subtree. Slices B–O read and invalidate through this one factory rather
// than inventing their own tuples; the work-item prefix nests beside the
// Issues area's entity lists (`issueKeys.lists`) so a module page and the
// module's own work-item reads stay distinguishable.

import { workspaceScope } from "../../../core/query/client.js";

export const moduleKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "modules"] as const,
  /** One project's non-archived modules (the list, board and timeline read). */
  list: (workspaceSlug: string, projectId: string) => [...moduleKeys.all(workspaceSlug), "list", projectId] as const,
  /** One module's detail (detail page and the peek body share this read). */
  detail: (workspaceSlug: string, projectId: string, moduleId: string) =>
    [...moduleKeys.all(workspaceSlug), "detail", projectId, moduleId] as const,
  /** One project's archived modules (the archived screen's own read). */
  archivedList: (workspaceSlug: string, projectId: string) =>
    [...moduleKeys.all(workspaceSlug), "archived", projectId] as const,
  /** One archived module for the archived peek. */
  archivedDetail: (workspaceSlug: string, projectId: string, moduleId: string) =>
    [...moduleKeys.archivedList(workspaceSlug, projectId), moduleId] as const,
  /** A module's links section (the sidebar's newest-first list). */
  links: (workspaceSlug: string, projectId: string, moduleId: string) =>
    [...moduleKeys.detail(workspaceSlug, projectId, moduleId), "links"] as const,
  /** A module's work-item pages, beneath the same prefix as its detail. */
  workItems: (workspaceSlug: string, projectId: string, moduleId: string) =>
    [...moduleKeys.detail(workspaceSlug, projectId, moduleId), "work-items"] as const,
  /** The workspace-wide module set pickers and switchers read. */
  workspaceList: (workspaceSlug: string) => [...moduleKeys.all(workspaceSlug), "workspace"] as const,
  /** The project name, switch and zone the list shell gates on (MOD-001/002). */
  projectFacts: (workspaceSlug: string, projectId: string) =>
    [...moduleKeys.all(workspaceSlug), "project-facts", projectId] as const,
  /** The project's estimate system, read by the sidebar's points row. */
  projectEstimates: (workspaceSlug: string, projectId: string) =>
    [...moduleKeys.all(workspaceSlug), "estimates", projectId] as const,
};
