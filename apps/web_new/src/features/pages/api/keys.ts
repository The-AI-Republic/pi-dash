// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages query keys. One collection entry per workspace/project serves the
// list, its tabs, the sibling switcher and the nested tree
// (parity/specs/pages.md §3.9, PAGE-020). The detail key starts here so
// later slices extend one tree instead of forking a second read.

import { workspaceScope } from "../../../core/query/client.js";

export const pageKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "pages"] as const,
  list: (workspaceSlug: string, projectId: string) => [...pageKeys.all(workspaceSlug), "list", projectId] as const,
  detail: (workspaceSlug: string, projectId: string, pageId: string) =>
    [...pageKeys.all(workspaceSlug), "detail", projectId, pageId] as const,
};
