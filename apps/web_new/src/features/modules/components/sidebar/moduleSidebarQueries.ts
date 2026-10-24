// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module detail and estimate reads behind the analytics sidebar
// (parity/specs/modules.md §9.1, §13.1, MOD-033). The detail is the same
// read the page and peek use, so it sits under `moduleKeys.detail` and
// follows the area's fetch-on-mount freshness. The estimate list is the
// project's own reference read, reused rather than invented: the sidebar
// only asks whether the active system is the points family.

import { queryOptions } from "@tanstack/react-query";
import {
  getArchivedModule,
  getModule,
  type ApiClient,
  type ArchivedModuleDetail,
  type ModuleDetail,
} from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../../core/query/client.js";
import { projectEstimatesQueryOptions } from "../../../estimates/index.js";
import { moduleKeys } from "../../api/keys.js";

/**
 * One module detail read for the sidebar. Active modules use the detail
 * key; the archived peek uses its own archived key and endpoint. The
 * return type is the union of the two records because the sidebar reads
 * only fields both carry; it never treats an archived row as editable.
 */
export function moduleSidebarDetailQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  moduleId: string,
  archived: boolean
) {
  return queryOptions<ModuleSidebarRecord>({
    queryKey: archived
      ? moduleKeys.archivedDetail(workspaceSlug, projectId, moduleId)
      : moduleKeys.detail(workspaceSlug, projectId, moduleId),
    queryFn: () =>
      archived
        ? getArchivedModule(client, workspaceSlug, projectId, moduleId)
        : getModule(client, workspaceSlug, projectId, moduleId),
    staleTime: LIST_STALE_TIME_MS,
    refetchOnWindowFocus: false,
  });
}

/** One module's full detail; the sidebar and its page share this key. */
export function moduleDetailQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  moduleId: string
) {
  return queryOptions({
    queryKey: moduleKeys.detail(workspaceSlug, projectId, moduleId),
    queryFn: () => getModule(client, workspaceSlug, projectId, moduleId),
    staleTime: LIST_STALE_TIME_MS,
    refetchOnWindowFocus: false,
  });
}

/** One archived module read for the archived peek's sidebar. */
export function archivedModuleDetailQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  moduleId: string
) {
  return queryOptions({
    queryKey: moduleKeys.archivedDetail(workspaceSlug, projectId, moduleId),
    queryFn: () => getArchivedModule(client, workspaceSlug, projectId, moduleId),
    staleTime: LIST_STALE_TIME_MS,
    refetchOnWindowFocus: false,
  });
}

/**
 * The project's estimate systems. The sidebar reuses the estimates
 * feature's read unchanged so a settings change that refreshes the
 * active system reaches the points gate too.
 */
export function moduleSidebarEstimatesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return projectEstimatesQueryOptions(client, workspaceSlug, projectId);
}

export type ModuleSidebarRecord = ModuleDetail | ArchivedModuleDetail;
