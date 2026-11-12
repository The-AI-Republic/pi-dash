// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace membership reads for the projects list filter panel
// (SHELL-029). The lead and member sections offer these people as
// options; the signed-in caller renders as "You".

import { queryOptions } from "@tanstack/react-query";
import { listWorkspaceMembers, type ApiClient } from "@pidash/api-client";

import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { projectKeys } from "./keys.js";

/**
 * Active memberships of one workspace. Reference data: names and
 * avatars barely move, so the shared reference freshness applies.
 */
export function workspaceMembersQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: projectKeys.members(workspaceSlug),
    queryFn: () => listWorkspaceMembers(client, workspaceSlug),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}
