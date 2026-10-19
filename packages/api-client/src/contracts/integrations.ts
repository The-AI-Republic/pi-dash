// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Code-hosting app connections for the integrations settings tab: the
// per-workspace installation status, starting an install handoff and a
// manual re-verification. Every call is scoped to the signed-in admin.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, uuid } from "./fields.js";

/**
 * One workspace's installation state. Disconnected is the minimal
 * `{ connected: false }` shape; connected rows carry the account and
 * verification fields the panel renders.
 */
export const GithubAppInstallation = z.object({
  connected: z.boolean(),
  installation_id: z.optional(z.number()),
  account_login: z.optional(z.string()),
  account_type: z.optional(z.string()),
  repository_selection: z.optional(z.string()),
  repository_count: z.optional(z.number()),
  permissions: z.optional(z.record(z.string(), z.unknown())),
  events: z.optional(z.array(z.string())),
  installed_at: z.optional(nullableDateTime()),
  suspended_at: z.optional(nullableDateTime()),
  verified_at: z.optional(nullableDateTime()),
  last_checked_at: z.optional(nullableDateTime()),
  last_check_error: z.optional(z.string()),
});
export type GithubAppInstallation = z.infer<typeof GithubAppInstallation>;

export const GithubAppWorkspace = z.object({
  id: uuid(),
  slug: z.string(),
  name: z.string(),
  github_app: GithubAppInstallation,
});
export type GithubAppWorkspace = z.infer<typeof GithubAppWorkspace>;

/** GET /api/users/me/integrations/github/app/ — configuration + workspaces. */
export const GithubAppStatus = z.object({
  configured: z.boolean(),
  app_slug: z.string(),
  workspaces: z.array(GithubAppWorkspace),
});
export type GithubAppStatus = z.infer<typeof GithubAppStatus>;

export async function getGithubAppStatus(client: ApiClient): Promise<GithubAppStatus> {
  const body = await client.getJson<unknown>("/api/users/me/integrations/github/app/");
  return client.parse(GithubAppStatus, body, "GET /api/users/me/integrations/github/app/");
}

/** The install handoff: a state token, its expiry and the host URL. */
export const GithubAppInstallStart = z.object({
  state: z.string(),
  expires_at: dateTime(),
  install_url: z.string(),
});
export type GithubAppInstallStart = z.infer<typeof GithubAppInstallStart>;

/**
 * POST /api/users/me/integrations/github/app/install/ with the workspace
 * slug. Answers 409 when the instance configuration is incomplete and
 * 403 for non-admins.
 */
export async function startGithubAppInstall(client: ApiClient, workspaceSlug: string): Promise<GithubAppInstallStart> {
  const path = "/api/users/me/integrations/github/app/install/";
  const envelope = await client.sendJson<unknown>("POST", path, { json: { workspace_slug: workspaceSlug } });
  return client.parse(GithubAppInstallStart, envelope.data, `POST ${path}`);
}

/**
 * POST /api/users/me/integrations/github/app/refresh/ with the workspace
 * slug. Answers the re-verified installation; 404 when nothing is
 * installed and 502 when the host rejects the check.
 */
export async function refreshGithubAppInstallation(
  client: ApiClient,
  workspaceSlug: string
): Promise<GithubAppInstallation> {
  const path = "/api/users/me/integrations/github/app/refresh/";
  const envelope = await client.sendJson<unknown>("POST", path, { json: { workspace_slug: workspaceSlug } });
  return client.parse(GithubAppInstallation, envelope.data, `POST ${path}`);
}
