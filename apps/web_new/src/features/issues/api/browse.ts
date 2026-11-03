// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Browse resolution, phase one (parity/specs/issues.md §13, ISS-142).
// The key in the route resolves through the lite identifier fetch,
// which answers for live, archived, draft, and intake items alike and
// tells the route which phase-two path to take: hydrate the full
// detail, or leave for the project intake tab. Unknown keys, refused
// keys, and malformed keys all resolve to null so the route renders
// one generic missing state, never a workspace-wide browser.

import { queryOptions } from "@tanstack/react-query";
import { getIssueByIdentifier, getIssueMeta, isApiError, type ApiClient, type IssueLite } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "./keys.js";

export interface LiteResolution {
  lite: IssueLite | null;
}

/**
 * Split a browse key into its project identifier and sequence number
 * (ACME-42). Identifiers may carry dashes themselves, so the split falls
 * on the last dash; anything without a numeric tail is not a key.
 */
export function parseBrowseKey(issueKey: string): { identifier: string; sequenceId: number } | null {
  const dash = issueKey.lastIndexOf("-");
  if (dash <= 0 || dash + 1 >= issueKey.length) return null;
  const sequenceId = Number(issueKey.slice(dash + 1));
  if (!Number.isInteger(sequenceId)) return null;
  return { identifier: issueKey.slice(0, dash), sequenceId };
}

/** True for the statuses that mean "no such key for this caller". */
function isUnknownKey(error: unknown): boolean {
  if (!isApiError(error)) return false;
  return error.status === 400 || error.status === 403 || error.status === 404;
}

/**
 * Resolve a browse key to its lite row (null when unknown). Server and
 * transport failures rethrow so the route can offer a retry.
 */
export async function loadLiteIssue(
  client: ApiClient,
  workspaceSlug: string,
  issueKey: string
): Promise<LiteResolution> {
  const parsed = parseBrowseKey(issueKey);
  if (parsed === null) return { lite: null };
  try {
    const lite = await getIssueByIdentifier(client, workspaceSlug, parsed.identifier, parsed.sequenceId);
    return { lite };
  } catch (error) {
    if (isUnknownKey(error)) return { lite: null };
    throw error;
  }
}

export function liteIssueQueryOptions(client: ApiClient, workspaceSlug: string, issueKey: string) {
  return queryOptions({
    queryKey: issueKeys.browse(workspaceSlug, issueKey),
    queryFn: () => loadLiteIssue(client, workspaceSlug, issueKey),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/**
 * Resolve a legacy short-link (project id plus issue id) to the browse
 * key the canonical route wants (ISS-143). Unknown, deleted, archived,
 * and refused issues all resolve to null: the route renders one generic
 * missing state and never distinguishes them.
 */
export async function resolveShortLink(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  issueId: string
): Promise<{ issueKey: string } | null> {
  const meta = await getIssueMeta(client, workspaceSlug, projectId, issueId).catch(() => null);
  if (meta === null) return null;
  return { issueKey: `${meta.project_identifier}-${meta.sequence_id}` };
}
