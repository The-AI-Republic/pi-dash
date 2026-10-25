// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module detail and estimate reads behind the analytics sidebar
// (parity/specs/modules.md §8.5, §9.1, §13.1, MOD-033, MOD-039). The
// detail is the same read the page and peek use, so it sits under
// `moduleKeys.detail` and is fetch-on-mount like every module read: the
// sidebar never picks up another member's rename on its own. The estimate list is the
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

import { FETCH_ON_MOUNT_FRESHNESS, fetchOnMountOptions } from "../../../issues/freshness/read.js";
import { projectEstimatesQueryOptions } from "../../../estimates/api.js";
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
  return queryOptions<ModuleSidebarRecord>(
    fetchOnMountOptions({
      queryKey: archived
        ? moduleKeys.archivedDetail(workspaceSlug, projectId, moduleId)
        : moduleKeys.detail(workspaceSlug, projectId, moduleId),
      queryFn: () =>
        archived
          ? getArchivedModule(client, workspaceSlug, projectId, moduleId)
          : getModule(client, workspaceSlug, projectId, moduleId),
    })
  );
}

/** One module's full detail; the sidebar and its page share this key. */
export function moduleDetailQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  moduleId: string
) {
  return queryOptions(
    fetchOnMountOptions({
      queryKey: moduleKeys.detail(workspaceSlug, projectId, moduleId),
      queryFn: () => getModule(client, workspaceSlug, projectId, moduleId),
    })
  );
}

/** One archived module read for the archived peek's sidebar. */
export function archivedModuleDetailQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  moduleId: string
) {
  return queryOptions(
    fetchOnMountOptions({
      queryKey: moduleKeys.archivedDetail(workspaceSlug, projectId, moduleId),
      queryFn: () => getArchivedModule(client, workspaceSlug, projectId, moduleId),
    })
  );
}

/**
 * The project's estimate systems, read only to decide whether the points
 * row shows. Same key and endpoint as the estimates feature so the cache
 * entry is shared, but the modules sidebar is fetch-on-mount (§8.5,
 * MOD-039): the estimates screen deliberately refetches on focus, and a
 * modules sidebar must not pick a settings change up that way.
 */
export function moduleSidebarEstimatesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return { ...projectEstimatesQueryOptions(client, workspaceSlug, projectId), ...FETCH_ON_MOUNT_FRESHNESS };
}

export type ModuleSidebarRecord = ModuleDetail | ArchivedModuleDetail;
