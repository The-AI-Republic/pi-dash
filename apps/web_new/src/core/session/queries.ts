// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Session queries: the current user, the workspaces they belong to, and
// the instance config the signed-out cards read. Route loaders prefetch
// these; components read them through the hooks.

import { queryOptions } from "@tanstack/react-query";
import {
  getInstanceConfig,
  getMe,
  getMeSettings,
  getProfile,
  listWorkspaces,
  type ApiClient,
  type Workspace,
  type WorkspaceList,
} from "@pidash/api-client";

import { REFERENCE_STALE_TIME_MS } from "../query/client.js";

export const sessionKeys = {
  me: ["session", "me"],
  workspaces: ["session", "workspaces"],
  profile: ["session", "profile"],
  settings: ["session", "settings"],
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

/** The user's profile (onboarding flags, preferences). Guards read the onboarded marker. */
export function profileQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: sessionKeys.profile,
    queryFn: () => getProfile(client),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

/** The signed-in user's settings (last/fallback workspace slugs for AUTH-016 landing). */
export function settingsQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: sessionKeys.settings,
    queryFn: () => getMeSettings(client),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

export const instanceKeys = {
  config: ["instance", "config"],
} as const;

/** Instance flags the auth cards and workspace surfaces read (AUTH-003/007/008/041). */
export function instanceConfigQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: instanceKeys.config,
    queryFn: () => getInstanceConfig(client),
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

export type WorkspaceGateDecision = "ok" | "sign-in" | "not-found";

/**
 * The `/$ws` route gate (SHELL-001, SHELL-108). Signed-out visitors pass
 * through: known pages bounce them to sign-in from the gated layout, while
 * the workspace catch-all renders the illustrated not-found page without a
 * session. Signed-in visitors who belong nowhere on this slug see the
 * workspace access notice. Pure, so routes and tests share it.
 */
export function decideWorkspaceRouteGate(args: {
  signedIn: boolean;
  workspaces: WorkspaceList | undefined;
  slugOrId: string;
}): WorkspaceGateDecision {
  if (!args.signedIn) return "ok";
  return selectWorkspace(args.workspaces, args.slugOrId) === undefined ? "not-found" : "ok";
}
