// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages query keys. One collection entry per workspace/project serves the
// list, its tabs, the sibling switcher and the nested tree
// (parity/specs/pages.md §3.9, PAGE-020). Detail, history and snapshot
// reads hang off the same root so a sign-out clears them together.

import { workspaceScope } from "../../../core/query/client.js";

export const pageKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "pages"] as const,
  list: (workspaceSlug: string, projectId: string) => [...pageKeys.all(workspaceSlug), "list", projectId] as const,
  detail: (workspaceSlug: string, projectId: string, pageId: string) =>
    [...pageKeys.all(workspaceSlug), "detail", projectId, pageId] as const,
  versions: (workspaceSlug: string, projectId: string, pageId: string) =>
    [...pageKeys.all(workspaceSlug), "versions", projectId, pageId] as const,
  version: (workspaceSlug: string, projectId: string, pageId: string, versionId: string) =>
    [...pageKeys.all(workspaceSlug), "version", projectId, pageId, versionId] as const,
};
