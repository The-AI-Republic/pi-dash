// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
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
