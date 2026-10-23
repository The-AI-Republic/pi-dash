// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Exports query keys (Data layer). Every key starts with the workspace
// scope so a workspace switch or sign-out clears one subtree.

import { workspaceScope } from "../../../core/query/client.js";

export const exportsKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "exports"] as const,
  /**
   * The cursor-paged history read. The history section (NEWFRONT-767) builds
   * its query on this key so the form's invalidation lands on it (EXP-006).
   */
  history: (workspaceSlug: string) => [...exportsKeys.all(workspaceSlug), "history"] as const,
};
