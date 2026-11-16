// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project query keys (Data layer). Every key starts with the workspace
// scope so a workspace switch or sign-out clears one subtree.

import { workspaceScope } from "../../../core/query/client.js";

export const projectKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "projects"] as const,
  list: (workspaceSlug: string) => [...projectKeys.all(workspaceSlug), "list"] as const,
  details: (workspaceSlug: string) => [...projectKeys.all(workspaceSlug), "details"] as const,
  /** Active workspace memberships (lead/member filter options, card avatars). */
  members: (workspaceSlug: string) => [...projectKeys.all(workspaceSlug), "members"] as const,
  /** The caller's workspace display properties (list mode, caps). */
  workspaceProperties: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "user-properties"] as const,
  /** The caller's per-project display properties (tab prefs, sort). */
  projectProperties: (workspaceSlug: string, projectId: string) =>
    [...workspaceScope(workspaceSlug), "projects", projectId, "user-properties"] as const,
};
