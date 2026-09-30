// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Session hooks (Data layer page). Components use these, never raw client
// calls or ad hoc role checks.

import { useQuery } from "@tanstack/react-query";

import { selectPermissions, type Permissions } from "./permissions.js";
import { getSessionClient, meQueryOptions, selectWorkspace, workspacesQueryOptions } from "./queries.js";
import { useSessionStore, type SessionStatus } from "./store.js";

/** The signed-in user. Suspends while loading when used with Suspense. */
export function useMe() {
  return useQuery(meQueryOptions(getSessionClient()));
}

/** All workspaces the user belongs to. */
export function useWorkspaces() {
  return useQuery(workspacesQueryOptions(getSessionClient()));
}

/** One workspace by slug or id, with the caller's role in it. */
export function useWorkspace(slugOrId: string) {
  const workspaces = useWorkspaces();
  const workspace = selectWorkspace(workspaces.data, slugOrId);
  return {
    ...workspaces,
    workspace,
    role: workspace?.role ?? null,
  };
}

/** Role checks for a workspace. The only place role numbers are read. */
export function usePermissions(slugOrId: string): Permissions & { isLoading: boolean } {
  const { role, isLoading } = useWorkspace(slugOrId);
  return { ...selectPermissions(role), isLoading };
}

/** The sign-in lifecycle state for redirects and the session-expired UI. */
export function useSessionStatus(): SessionStatus {
  return useSessionStore((state) => state.status);
}
