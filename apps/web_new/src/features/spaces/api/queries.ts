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
  getSpaceBoardIssue,
  getSpaceBoardMeta,
  getSpaceBoardSettings,
  listSpaceBoardIssues,
  listSpaceComments,
  listSpaceBoardLabels,
  listSpaceBoardStates,
  resolveSpaceBoardAnchor,
  type ApiClient,
  type Me,
  type SpaceBoardIssueDetail,
  type SpaceBoardIssuePage,
  type SpaceBoardLabel,
  type SpaceBoardMeta,
  type SpaceBoardSettings,
  type SpaceBoardState,
  type SpaceComment,
} from "@pidash/api-client";

import { isValidAnchor, type SpaceBoardFilters } from "../lib/address.js";
import { isSpaceIssueId } from "../lib/peek.js";
import { spaceBoardQuery, spaceFilterSignature } from "../layouts/model.js";
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
  } catch {
    return null;
  }
}

export function spaceViewerQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: [...spaceKeys.all, "viewer"] as const,
    queryFn: () => readSpaceViewer(client),
    staleTime: 60_000,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    retry: false,
  });
}

export function spaceSettingsQueryOptions(client: ApiClient, anchor: string) {
  return queryOptions({
    queryKey: spaceKeys.settings(anchor),
    queryFn: () => getSpaceBoardSettings(client, anchor),
    enabled: isValidAnchor(anchor),
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    retry: false,
  });
}

/**
 * The open issue's public comment thread (SPC-049/050). The endpoint
 * already filters to EXTERNAL comments and orders by creation; the section
 * stays behind the publisher's comments flag at the component level. Gated
 * on a uuid so a malformed peek address never becomes a request.
 */
export function spaceCommentsQueryOptions(client: ApiClient, anchor: string, issueId: string) {
  return queryOptions({
    queryKey: spaceKeys.comments(anchor, issueId),
    queryFn: () => listSpaceComments(client, anchor, issueId),
    enabled: isValidAnchor(anchor) && isSpaceIssueId(issueId),
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    retry: false,
  });
}

/**
 * The board's first page: one bucket of 50 per workflow state, filtered by
 * the address's facets (SPC-026). Layout is not part of the key, so switching
 * layout re-reads nothing.
 */
export function spaceIssuesQueryOptions(client: ApiClient, anchor: string, filters: SpaceBoardFilters = {}) {
  const signature = spaceFilterSignature(filters);
  return queryOptions({
    queryKey: spaceKeys.issues(anchor, signature),
    queryFn: () => listSpaceBoardIssues(client, anchor, spaceBoardQuery(filters)),
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    retry: false,
  });
}

/**
 * One group's later page, addressed by that group's opaque cursor. Keyed by
 * state and cursor so two triggers for the same page share one request and
 * cannot duplicate rows.
 */
export function spaceGroupQueryOptions(
  client: ApiClient,
  anchor: string,
  filters: SpaceBoardFilters,
  stateId: string,
  cursor: string
) {
  const signature = spaceFilterSignature(filters);
  return queryOptions({
    queryKey: [...spaceKeys.group(anchor, stateId, signature), cursor] as const,
    queryFn: () => listSpaceBoardIssues(client, anchor, spaceBoardQuery(filters, cursor, stateId)),
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    retry: false,
    staleTime: Number.POSITIVE_INFINITY,
  });
}

/**
 * One issue's detail for the open overlay (SPC-040/043/046). Gated on a
 * uuid so a malformed `peekId` never becomes a request; the board behind
 * the overlay is untouched while this resolves, and neither focus nor a
 * reconnect re-reads it (SPC-033).
 */
export function spaceIssueDetailQueryOptions(client: ApiClient, anchor: string, issueId: string) {
  return queryOptions({
    queryKey: spaceKeys.detail(anchor, issueId),
    queryFn: () => getSpaceBoardIssue(client, anchor, issueId),
    enabled: isValidAnchor(anchor) && isSpaceIssueId(issueId),
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    retry: false,
  });
}

export function spaceStatesQueryOptions(client: ApiClient, anchor: string) {
  return queryOptions({
    queryKey: spaceKeys.states(anchor),
    queryFn: () => listSpaceBoardStates(client, anchor),
    staleTime: 5 * 60_000,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    retry: false,
  });
}

/**
 * Label names and colours for the cards' chips (SPC-028). Display data is
 * read once per visit; the board never polls or revalidates it on focus.
 */
export function spaceLabelsQueryOptions(client: ApiClient, anchor: string) {
  return queryOptions({
    queryKey: spaceKeys.labels(anchor),
    queryFn: () => listSpaceBoardLabels(client, anchor),
    enabled: isValidAnchor(anchor),
    staleTime: 5 * 60_000,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    retry: false,
  });
}

/**
 * Board display metadata for the document head (SPC-007). Gated on a
 * valid anchor so a malformed value never produces a request; unknown
 * anchors answer 404 and the head falls back to the generic tags.
 */
export function spaceMetaQueryOptions(client: ApiClient, anchor: string) {
  return queryOptions({
    queryKey: spaceKeys.meta(anchor),
    queryFn: () => getSpaceBoardMeta(client, anchor),
    enabled: isValidAnchor(anchor),
    staleTime: 5 * 60_000,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
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

export type {
  Me,
  SpaceBoardIssueDetail,
  SpaceBoardIssuePage,
  SpaceBoardLabel,
  SpaceBoardMeta,
  SpaceBoardSettings,
  SpaceBoardState,
  SpaceComment,
};
