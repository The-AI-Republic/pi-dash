// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Project queries. Route loaders prefetch these; components read them
// through hooks, never through the HTTP client directly.

import { queryOptions } from "@tanstack/react-query";
import { listProjects, type ApiClient } from "@pidash/api-client";

import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { projectKeys } from "./keys.js";

export function projectsQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: projectKeys.list(workspaceSlug),
    queryFn: () => listProjects(client, workspaceSlug),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}
