// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stored description versions (parity/specs/issues.md §12, ISS-147). The
// history panel lists an item's saved versions and reads one back when the
// user previews or restores it. Only the detail pane's history uses these
// reads, so they live apart from the list/detail options the first route
// needs; the api-client version contract rides this module's chunk
// (NEWFRONT-871, size gate).

import { queryOptions } from "@tanstack/react-query";
import { getDescriptionVersion, listDescriptionVersions, type ApiClient } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "./keys.js";

/** Stored description versions, newest first (ISS-147). */
export function descriptionVersionsQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  issueId: string
) {
  return queryOptions({
    queryKey: issueKeys.descriptionVersions(workspaceSlug, projectId, issueId),
    queryFn: () => listDescriptionVersions(client, workspaceSlug, projectId, issueId),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** One stored version with the HTML a restore writes back (ISS-147). */
export function descriptionVersionQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  versionId: string
) {
  return queryOptions({
    queryKey: issueKeys.descriptionVersion(workspaceSlug, projectId, issueId, versionId),
    queryFn: () => getDescriptionVersion(client, workspaceSlug, projectId, issueId, versionId),
    staleTime: LIST_STALE_TIME_MS,
  });
}
