// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project repository binding (parity/specs/project-settings-general.md §8,
// project-settings-part-f.md §PSET-050/051): the General tab's bind row and
// the Repository tab's status card, sync switch and unbind control. The
// endpoint answers an unbound marker (`{ bound: false }`) when the project
// has no binding, and a full binding row once one exists.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { nullableDateTime, nullableUuid, uuid } from "./fields.js";

export const REPOSITORY_PROVIDERS = ["github", "gitlab"] as const;

/** Clone credential strategy the server chose for the binding. */
export const REPOSITORY_CLONE_AUTH_MODES = ["public", "runner_managed", "managed_ephemeral"] as const;

/** The provider-side repository behind a binding. */
export const BoundRepository = z.object({
  id: z.string(),
  provider: z.enum(REPOSITORY_PROVIDERS),
  host_url: z.string(),
  namespace: z.string(),
  name: z.string(),
  full_name: z.string(),
  web_url: z.string(),
  clone_url_http: z.string(),
  clone_url_ssh: z.string(),
  default_branch: z.string(),
  private: z.boolean(),
});
export type BoundRepository = z.infer<typeof BoundRepository>;

/**
 * A project's live binding. `degraded` mirrors the provider account's
 * status; `degraded_reason` carries its last check error (null while
 * healthy). `last_synced_at` is null before the first successful sync.
 */
export const ProjectRepositoryBinding = z.object({
  bound: z.literal(true),
  id: uuid(),
  provider: z.enum(REPOSITORY_PROVIDERS),
  provider_account_id: nullableUuid(),
  host_url: z.string(),
  repository: BoundRepository,
  is_sync_enabled: z.boolean(),
  clone_auth_mode: z.enum(REPOSITORY_CLONE_AUTH_MODES),
  last_synced_at: nullableDateTime(),
  last_sync_error: z.string(),
  degraded: z.boolean(),
  degraded_reason: z.nullable(z.string()),
});
export type ProjectRepositoryBinding = z.infer<typeof ProjectRepositoryBinding>;

/** GET on a project with no binding answers only this marker. */
export const ProjectRepositoryUnbound = z.object({
  bound: z.literal(false),
});
export type ProjectRepositoryUnbound = z.infer<typeof ProjectRepositoryUnbound>;

export const ProjectRepositoryStatus = z.union([ProjectRepositoryBinding, ProjectRepositoryUnbound]);
export type ProjectRepositoryStatus = z.infer<typeof ProjectRepositoryStatus>;

/** GET .../projects/<pid>/repository/ — any project member. */
export async function getProjectRepository(
  client: ApiClient,
  slug: string,
  projectId: string
): Promise<ProjectRepositoryStatus> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/repository/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(ProjectRepositoryStatus, body, `GET ${path}`);
}

export interface RepositoryBindInput {
  /** Repository web URL (GitHub or GitLab); the server parses it. */
  repo_url: string;
  /** Pick one provider account when several can see the same host. */
  provider_account_id?: string | null;
}

/**
 * The bind answer is the binding row plus the canonical `repo_url` the
 * project should now store. POST answers 201. Binding requires a connected
 * provider account: a workspace without one gets 409 ProviderAccountRequired,
 * and an unknown repository URL gets 400.
 */
export const ProjectRepositoryBound = z.extend(ProjectRepositoryBinding, {
  repo_url: z.string(),
});
export type ProjectRepositoryBound = z.infer<typeof ProjectRepositoryBound>;

/** POST .../repository/bind/ — project admin only. */
export async function bindProjectRepository(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: RepositoryBindInput
): Promise<ProjectRepositoryBound> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/repository/bind/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(ProjectRepositoryBound, envelope.data, `POST ${path}`);
}

/**
 * PATCH .../repository/ with the sync switch. The server requires a real
 * boolean and a live binding (404 when unbound); the answer is the binding
 * row with the new `is_sync_enabled` value.
 */
export async function setProjectRepositorySync(
  client: ApiClient,
  slug: string,
  projectId: string,
  enabled: boolean
): Promise<ProjectRepositoryBinding> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/repository/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: { enabled } });
  return client.parse(ProjectRepositoryBinding, envelope.data, `PATCH ${path}`);
}

/**
 * DELETE .../repository/ drops the binding and its GitHub sync rows and
 * answers 200 with the unbound marker. Already-unbound projects answer the
 * same marker.
 */
export async function unbindProjectRepository(
  client: ApiClient,
  slug: string,
  projectId: string
): Promise<ProjectRepositoryUnbound> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/repository/`;
  const envelope = await client.sendJson<unknown>("DELETE", path, { json: {} });
  return client.parse(ProjectRepositoryUnbound, envelope.data, `DELETE ${path}`);
}
