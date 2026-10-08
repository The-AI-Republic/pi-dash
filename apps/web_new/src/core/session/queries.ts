// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Session queries: the current user and the workspaces they belong to.
// Route loaders prefetch these; components read them through the hooks.

import { queryOptions } from "@tanstack/react-query";
import { getMe, listWorkspaces, type ApiClient, type Workspace, type WorkspaceList } from "@pidash/api-client";

import { REFERENCE_STALE_TIME_MS } from "../query/client.js";

export const sessionKeys = {
  me: ["session", "me"],
  workspaces: ["session", "workspaces"],
} as const;

export function meQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: sessionKeys.me,
    queryFn: () => getMe(client),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

export function workspacesQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: sessionKeys.workspaces,
    queryFn: () => listWorkspaces(client),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

let sessionClient: ApiClient | null = null;

/** Called once at bootstrap; hooks read the client from here. */
export function setSessionClient(client: ApiClient): void {
  sessionClient = client;
}

export function getSessionClient(): ApiClient {
  if (!sessionClient) {
    throw new Error("Session client is not configured yet");
  }
  return sessionClient;
}

/** Find a workspace by id or slug. Pure, so routes and tests share it. */
export function selectWorkspace(workspaces: WorkspaceList | undefined, slugOrId: string): Workspace | undefined {
  return workspaces?.find((workspace) => workspace.slug === slugOrId || workspace.id === slugOrId);
}

/** The caller's role in the workspace, or null outside it. */
export function selectWorkspaceRole(workspaces: WorkspaceList | undefined, slugOrId: string): number | null {
  return selectWorkspace(workspaces, slugOrId)?.role ?? null;
}
