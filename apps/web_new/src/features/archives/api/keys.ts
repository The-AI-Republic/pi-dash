// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archives query keys (Data layer). Every key starts with the workspace
// scope so a workspace switch or sign-out clears one subtree. Tab slices
// nest their list keys under `all` next to the header count, so one
// invalidation refreshes the tab and its badge together.

import { workspaceScope } from "../../../core/query/client.js";

export const archivesKeys = {
  all: (workspaceSlug: string, projectId: string) => [...workspaceScope(workspaceSlug), "archives", projectId] as const,
  /** Total archived work items feeding the header badge (ARCH-002). */
  count: (workspaceSlug: string, projectId: string) =>
    [...archivesKeys.all(workspaceSlug, projectId), "count"] as const,
};
