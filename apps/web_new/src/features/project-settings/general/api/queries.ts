// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reads behind the General tab (parity/specs/project-settings-general.md
// §PSET-005, §PSET-008): the project's settings row, the repository binding
// status and the live identifier availability answer. One settings key per
// workspace/project; writes re-seed it so the form, the page header and the
// rail agree without a reload.

import { useQuery, type QueryClient } from "@tanstack/react-query";
import {
  checkProjectIdentifier,
  getProjectSettingsDetail,
  getProjectRepository,
  type ApiClient,
  type ProjectRepositoryStatus,
  type ProjectSettingsDetail,
} from "@pidash/api-client";

import { getAppClient } from "../../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../../../core/query/index.js";
import { authKeys } from "../../../auth/index.js";
import { projectKeys } from "../../../projects/index.js";

export const generalKeys = {
  detail: (slug: string, projectId: string) => [...projectKeys.all(slug), "settings", projectId, "general"] as const,
  repository: (slug: string, projectId: string) =>
    [...projectKeys.all(slug), "settings", projectId, "repository"] as const,
  identifier: (slug: string, name: string) =>
    [...projectKeys.all(slug), "settings", "identifier", name.toUpperCase()] as const,
};

export function generalDetailQueryOptions(client: ApiClient, slug: string, projectId: string) {
  return {
    queryKey: generalKeys.detail(slug, projectId),
    queryFn: () => getProjectSettingsDetail(client, slug, projectId),
    staleTime: REFERENCE_STALE_TIME_MS,
  };
}

export function repositoryStatusQueryOptions(client: ApiClient, slug: string, projectId: string) {
  return {
    queryKey: generalKeys.repository(slug, projectId),
    queryFn: () => getProjectRepository(client, slug, projectId),
    staleTime: REFERENCE_STALE_TIME_MS,
  };
}

export function identifierQueryOptions(client: ApiClient, slug: string, name: string) {
  return {
    queryKey: generalKeys.identifier(slug, name),
    queryFn: () => checkProjectIdentifier(client, slug, name),
    staleTime: 0,
    gcTime: 0,
  };
}

/** The tab's row; it also seeds the shell's project read so a rename propagates. */
export function useGeneralDetail(slug: string, projectId: string) {
  return useQuery(generalDetailQueryOptions(getAppClient(), slug, projectId));
}

/** Binding status; only the bind flow and a matching URL read it. */
export function useRepositoryStatus(slug: string, projectId: string, enabled: boolean) {
  return useQuery({ ...repositoryStatusQueryOptions(getAppClient(), slug, projectId), enabled });
}

/** Live availability only runs for a changed, non-empty code. */
export function useIdentifierCheck(slug: string, name: string | null) {
  return useQuery({
    ...identifierQueryOptions(getAppClient(), slug, name ?? ""),
    enabled: name !== null,
  });
}

/**
 * Write a freshly read settings row into every cache the page reads: the
 * tab's own key, the shell's `getProject` row (name, emblem, identifier)
 * and its exact invalidation so navigation re-reads the collection.
 */
export function seedProjectRow(client: QueryClient, slug: string, projectId: string, row: ProjectSettingsDetail): void {
  client.setQueryData(generalKeys.detail(slug, projectId), row);
  client.setQueryData<ProjectSettingsDetail>(authKeys.project(slug, projectId), (previous) => ({
    ...(previous ?? row),
    name: row.name,
    identifier: row.identifier,
    logo_props: row.logo_props,
  }));
  void client.invalidateQueries({ queryKey: projectKeys.all(slug) });
}

/** Read binding status once, outside a component (the bind flow). */
export function readRepositoryStatus(
  client: QueryClient,
  api: ApiClient,
  slug: string,
  projectId: string
): Promise<ProjectRepositoryStatus> {
  return client.fetchQuery(repositoryStatusQueryOptions(api, slug, projectId));
}
