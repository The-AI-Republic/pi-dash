// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages detail reads (parity/specs/pages.md §4.1, §4.5, §11.1). The record
// read carries visit tracking; the collection read is the same cached
// query the list and sibling switcher use, and the version reads feed the
// snapshot overlay. A stopped query never flashes a stale editor.

import { getPage, getPageVersion, listPageVersions, type ApiClient } from "@pidash/api-client";
import { queryOptions } from "@tanstack/react-query";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { pageKeys } from "./keys.js";

export function pageDetailQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, pageId: string) {
  return queryOptions({
    queryKey: pageKeys.detail(workspaceSlug, projectId, pageId),
    queryFn: () => getPage(client, workspaceSlug, projectId, pageId, { trackVisit: true }),
    staleTime: LIST_STALE_TIME_MS,
    retry: false,
  });
}

export function pageVersionsQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, pageId: string) {
  return queryOptions({
    queryKey: pageKeys.versions(workspaceSlug, projectId, pageId),
    queryFn: () => listPageVersions(client, workspaceSlug, projectId, pageId),
    staleTime: LIST_STALE_TIME_MS,
    retry: false,
  });
}

export function pageVersionQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  pageId: string,
  versionId: string
) {
  return queryOptions({
    queryKey: pageKeys.version(workspaceSlug, projectId, pageId, versionId),
    queryFn: () => getPageVersion(client, workspaceSlug, projectId, pageId, versionId),
    staleTime: LIST_STALE_TIME_MS,
    retry: false,
  });
}
