// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace details read for the settings frame (parity/specs/
// workspace-settings.md §1, §38; WSET-001/038): the sidebar names the
// workspace and shows its logo, and the read follows the settings fetch
// policy — once on mount, never from a returning focus or a reconnect.

import { queryOptions } from "@tanstack/react-query";

import { getWorkspace, type ApiClient, type WorkspaceDetails } from "@pidash/api-client";
import { workspaceScope } from "../../../../../core/query/index.js";
import { WORKSPACE_SETTINGS_QUERY_POLICY } from "../../../../../features/workspace-settings/index.js";

export function workspaceSettingsDetailsKey(workspaceSlug: string): readonly unknown[] {
  return [...workspaceScope(workspaceSlug), "workspace-settings", "details"];
}

export function workspaceQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions<WorkspaceDetails>({
    queryKey: workspaceSettingsDetailsKey(workspaceSlug),
    queryFn: () => getWorkspace(client, workspaceSlug),
    ...WORKSPACE_SETTINGS_QUERY_POLICY,
  });
}
