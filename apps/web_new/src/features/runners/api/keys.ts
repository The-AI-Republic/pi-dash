// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners cache keys (NEWFRONT-540). Every key starts inside the workspace
// subtree so a workspace switch or sign-out drops the area wholesale. The
// scope segment is the project id, or `"workspace"` for the aggregate.

import { workspaceScope } from "../../../core/query/client.js";

/** `null` scope is the workspace-wide aggregate. */
export type RunnersScope = string | null;

function scopeSegment(projectId: RunnersScope): string {
  return projectId ?? "workspace";
}

export const runnersKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "runners"] as const,
  list: (workspaceSlug: string, projectId: RunnersScope) =>
    [...runnersKeys.all(workspaceSlug), "list", scopeSegment(projectId)] as const,
  pods: (workspaceSlug: string, projectId: RunnersScope) =>
    [...runnersKeys.all(workspaceSlug), "pods", scopeSegment(projectId)] as const,
  runs: (workspaceSlug: string, projectId: RunnersScope, page: number) =>
    [...runnersKeys.all(workspaceSlug), "runs", scopeSegment(projectId), page] as const,
  run: (workspaceSlug: string, runId: string) => [...runnersKeys.all(workspaceSlug), "run", runId] as const,
};
