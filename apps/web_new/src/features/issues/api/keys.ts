// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue query keys (Data layer). Every key starts with the workspace
// scope so a workspace switch or sign-out clears one subtree. The list
// key carries the project and the validated view search object.

import { workspaceScope } from "../../../core/query/client.js";
import type { IssueSearch } from "../filters/search.js";

export const issueKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "issues"] as const,
  list: (workspaceSlug: string, projectId: string, search: IssueSearch) =>
    [...issueKeys.all(workspaceSlug), "list", projectId, search] as const,
  detail: (workspaceSlug: string, issueId: string) => [...issueKeys.all(workspaceSlug), "detail", issueId] as const,
  browse: (workspaceSlug: string, issueKey: string) => [...issueKeys.all(workspaceSlug), "browse", issueKey] as const,
  /** Description version history of one work item (ISS-147). */
  descriptionVersions: (workspaceSlug: string, projectId: string, issueId: string) =>
    [...issueKeys.all(workspaceSlug), "description-versions", projectId, issueId] as const,
  /** One stored description version, with its HTML payload (ISS-147). */
  descriptionVersion: (workspaceSlug: string, projectId: string, issueId: string, versionId: string) =>
    [...issueKeys.descriptionVersions(workspaceSlug, projectId, issueId), versionId] as const,
  /**
   * Per-entity issue-list prefix (parity/specs/issues.md §1, ISS-002). Every
   * entity list query nests beneath its prefix so a display-preference write
   * refetches exactly the lists it affects: project lists live under
   * `lists(slug, projectId)` (the flat list already does), other entities
   * under a scope tag plus their ids. Project ids are UUIDs, so they never
   * collide with the scope tags.
   */
  lists: (workspaceSlug: string, ...part: string[]) => [...issueKeys.all(workspaceSlug), "list", ...part] as const,
  /** Per-entity display preferences (ISS-002). */
  prefs: (workspaceSlug: string, ...part: string[]) => [...issueKeys.all(workspaceSlug), "prefs", ...part] as const,
};
