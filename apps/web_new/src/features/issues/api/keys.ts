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
};
