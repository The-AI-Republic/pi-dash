// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public-board reads (parity/specs/space.md §5.1). Every read goes through
// the public runtime, so a 401 is an anonymous visitor and never expires a
// workspace session. Query functions stay pure enough to unit-test through
// their options.

import { queryOptions } from "@tanstack/react-query";
import {
  getInstanceBootstrapForStartup,
  getMe,
  getSpaceBoardSettings,
  isApiError,
  listSpaceBoardIssues,
  listSpaceBoardStates,
  resolveSpaceBoardAnchor,
  type ApiClient,
  type Me,
  type SpaceBoardIssuePage,
  type SpaceBoardSettings,
  type SpaceBoardState,
} from "@pidash/api-client";

import { isValidAnchor } from "../lib/address.js";
import { spaceKeys } from "./keys.js";

/** The public instance probe: one attempt, no client retries. */
export function spaceInstanceQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: spaceKeys.instance,
    queryFn: () => getInstanceBootstrapForStartup(client),
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    retry: false,
  });
}

/** Anonymous visitors get null; the board never waits on identity. */
export async function readSpaceViewer(client: ApiClient): Promise<Me | null> {
  try {
    return await getMe(client);
  } catch (error) {
    if (isApiError(error) && error.status === 401) return null;
    return null;
  }
}

export function spaceViewerQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: [...spaceKeys.all, "viewer"] as const,
    queryFn: () => readSpaceViewer(client),
    staleTime: 60_000,
    retry: false,
  });
}

export function spaceSettingsQueryOptions(client: ApiClient, anchor: string) {
  return queryOptions({
    queryKey: spaceKeys.settings(anchor),
    queryFn: () => getSpaceBoardSettings(client, anchor),
    enabled: isValidAnchor(anchor),
    retry: false,
  });
}

export function spaceIssuesQueryOptions(client: ApiClient, anchor: string) {
  return queryOptions({
    queryKey: spaceKeys.issues(anchor),
    queryFn: () => listSpaceBoardIssues(client, anchor, { groupBy: "state_id", perPage: 50 }),
    retry: false,
  });
}

export function spaceStatesQueryOptions(client: ApiClient, anchor: string) {
  return queryOptions({
    queryKey: spaceKeys.states(anchor),
    queryFn: () => listSpaceBoardStates(client, anchor),
    staleTime: 5 * 60_000,
    retry: false,
  });
}

/**
 * Resolve a legacy project address. Anything the public endpoint refuses
 * (unknown, unpublished, malformed id, non-project payload) is null and
 * lands on the generic not-found page; a transient failure also falls back
 * there rather than showing workspace chrome.
 */
export async function resolveLegacySpaceAnchor(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string
): Promise<string | null> {
  try {
    const settings: SpaceBoardSettings = await resolveSpaceBoardAnchor(client, workspaceSlug, projectId);
    if (settings.entity_name !== "project" || settings.anchor === null || !isValidAnchor(settings.anchor)) return null;
    return settings.anchor;
  } catch {
    return null;
  }
}

export type { Me, SpaceBoardIssuePage, SpaceBoardSettings, SpaceBoardState };
