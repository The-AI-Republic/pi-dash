// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runtime queries (RUN-045). The enrollment pill polls one project row
// until the server reports the managed runner available; the executor
// picker reuses this same key later so both read one cached row.

import { queryOptions } from "@tanstack/react-query";
import { getProject, type ApiClient, type ProjectDetail } from "@pidash/api-client";

import { workspaceScope } from "../../../core/query/client.js";
import { MANAGED_RUNNER_KIND } from "./service.js";

export const runtimeKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "runners", "runtime"] as const,
  projectAvailability: (workspaceSlug: string, projectId: string) =>
    [...runtimeKeys.all(workspaceSlug), "project-availability", projectId] as const,
};

/** One project row, polled by the enrollment pill for managed-runner availability. */
export function projectAvailabilityQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: runtimeKeys.projectAvailability(workspaceSlug, projectId),
    queryFn: () => getProject(client, workspaceSlug, projectId),
    staleTime: 0,
  });
}

/** True when the server reports the managed runner available on this project. */
export function isManagedRunnerAvailable(project: ProjectDetail | undefined): boolean {
  return (
    project?.agent_executor_options.some((option) => option.kind === MANAGED_RUNNER_KIND && option.available) ?? false
  );
}
