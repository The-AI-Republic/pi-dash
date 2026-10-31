// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace details read for the settings area
// (parity/specs/workspace-settings.md §1, §3, §38; WSET-001/003/038).
// The frame's sidebar and the general form read one row through one key,
// so a save updates both: the key sits under the workspace scope and the
// fetch policy re-reads only on mount.

import { queryOptions } from "@tanstack/react-query";

import { getWorkspace, type ApiClient, type WorkspaceDetails } from "@pidash/api-client";
import { workspaceScope } from "../../../../core/query/index.js";
import { WORKSPACE_SETTINGS_QUERY_POLICY } from "../../lib/settingsQuery.js";

export function workspaceDetailsKey(workspaceSlug: string): readonly unknown[] {
  return [...workspaceScope(workspaceSlug), "workspace-settings", "details"];
}

export function workspaceDetailsQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions<WorkspaceDetails>({
    queryKey: workspaceDetailsKey(workspaceSlug),
    queryFn: () => getWorkspace(client, workspaceSlug),
    ...WORKSPACE_SETTINGS_QUERY_POLICY,
  });
}
