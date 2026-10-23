// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views query keys (Data layer). Every key starts with the workspace
// scope so a workspace switch or sign-out clears one subtree.

import { workspaceScope } from "../../../core/query/client.js";

export const viewKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "views"] as const,
  /** One project's saved-views collection (VIEW-001). */
  projectList: (workspaceSlug: string, projectId: string) =>
    [...viewKeys.all(workspaceSlug), "project", projectId, "list"] as const,
  /**
   * One project view's detail record. The tail deliberately matches the
   * issues filter row's saved-view key, so a dialog's PATCH echo and a
   * row action's cache eviction reach the same entry the detail reads.
   */
  projectDetail: (workspaceSlug: string, projectId: string, viewId: string) =>
    [...workspaceScope(workspaceSlug), "issues", "saved-view", projectId, viewId] as const,
  /** The workspace's custom saved views, defaults excluded (VIEW-029). */
  workspaceList: (workspaceSlug: string) => [...viewKeys.all(workspaceSlug), "workspace", "list"] as const,
};
