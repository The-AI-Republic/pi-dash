// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Code-hosting app connections for the integrations settings tab: the
// per-workspace installation status, starting an install handoff and a
// manual re-verification. Every call is scoped to the signed-in admin.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";

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

// Workspace developer-integration contracts (WSET-016-018): the GitHub
// provider's connection state, and the Git provider-account rows the
// GitLab card lists. The legacy provider/repository/import endpoints are
// dead per the workspace-settings spec §42 and stay out of this module.

/**
 * GitHub provider status. A disconnected workspace answers only the flag;
 * a connected one adds the verified login and verification time. Reads are
 * allowed for admins and members; writes are admin-only server-side.
 */
export const GithubIntegrationStatus = z.object({
  connected: z.boolean(),
  github_user_login: z.optional(z.nullable(z.string())),
  verified_at: z.optional(nullableDateTime()),
});
export type GithubIntegrationStatus = z.infer<typeof GithubIntegrationStatus>;

/** GET /api/workspaces/<slug>/integrations/github/ — the card's state read. */
export async function getGithubIntegration(client: ApiClient, slug: string): Promise<GithubIntegrationStatus> {
  const path = `/api/workspaces/${slug}/integrations/github/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(GithubIntegrationStatus, body, `GET ${path}`);
}

export interface GithubConnectInput {
  /** Fine-grained personal access token, trimmed by the caller. */
  token: string;
}

/**
 * POST /api/workspaces/<slug>/integrations/github/connect/ with `{ token }`.
 * The backend verifies the credential against GitHub before storing it, so
 * a rejected token answers 401 and a blank one 400. Answers the connected
 * status shape.
 */
export async function connectGithubIntegration(
  client: ApiClient,
  slug: string,
  input: GithubConnectInput
): Promise<GithubIntegrationStatus> {
  const path = `/api/workspaces/${slug}/integrations/github/connect/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(GithubIntegrationStatus, envelope.data, `POST ${path}`);
}

/**
 * POST /api/workspaces/<slug>/integrations/github/disconnect/ with an empty
 * body. Soft-disconnects: project bindings stay but stop syncing, and the
 * linked PAT account is revoked. Answers `{ connected: false }`.
 */
export async function disconnectGithubIntegration(client: ApiClient, slug: string): Promise<GithubIntegrationStatus> {
  const path = `/api/workspaces/${slug}/integrations/github/disconnect/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: {} });
  return client.parse(GithubIntegrationStatus, envelope.data, `POST ${path}`);
}

export const GIT_ACCOUNT_PROVIDERS = ["github", "gitlab"] as const;
export type GitAccountProvider = (typeof GIT_ACCOUNT_PROVIDERS)[number];

export const GIT_ACCOUNT_STATUSES = ["connected", "degraded", "revoked", "error"] as const;
export type GitAccountStatus = (typeof GIT_ACCOUNT_STATUSES)[number];

export const GIT_ACCOUNT_AUTH_TYPES = ["github_app", "pat", "oauth", "group_token", "project_token"] as const;
export type GitAccountAuthType = (typeof GIT_ACCOUNT_AUTH_TYPES)[number];

/**
 * One provider account row. Credential material never crosses this
 * boundary: only the account identity, host, status, and the verified
 * capability flags the card can explain.
 */
export const GitProviderAccount = z.object({
  id: uuid(),
  provider: z.enum(GIT_ACCOUNT_PROVIDERS),
  host_url: z.string(),
  auth_type: z.enum(GIT_ACCOUNT_AUTH_TYPES),
  external_account_id: z.string(),
  external_account_login: z.string(),
  display_name: z.string(),
  capabilities: z.record(z.string(), z.boolean()),
  status: z.enum(GIT_ACCOUNT_STATUSES),
  verified_at: nullableDateTime(),
  last_check_error: z.string(),
});
export type GitProviderAccount = z.infer<typeof GitProviderAccount>;

export const GitProviderAccountList = z.object({
  accounts: z.array(GitProviderAccount),
});
export type GitProviderAccountList = z.infer<typeof GitProviderAccountList>;

/**
 * GET /api/workspaces/<slug>/integrations/git/accounts/ — every provider
 * account in the workspace. The GitLab card picks the first non-revoked
 * account as its active one; the list itself is unfiltered.
 */
export async function listGitProviderAccounts(client: ApiClient, slug: string): Promise<GitProviderAccountList> {
  const path = `/api/workspaces/${slug}/integrations/git/accounts/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(GitProviderAccountList, body, `GET ${path}`);
}

export interface RegisterGitProviderAccountInput {
  provider: GitAccountProvider;
  token: string;
  /** Only sent when the form holds a non-blank host. */
  host_url?: string;
  /** The UI always registers personal tokens. */
  auth_type?: Extract<GitAccountAuthType, "pat">;
}

/**
 * POST /api/workspaces/<slug>/integrations/git/accounts/ with the provider,
 * token, optional host, and personal-token auth type. The backend verifies
 * the credential with the provider first; a rejection surfaces as 401/403
 * and stores nothing. Answers the created account row (201).
 */
export async function registerGitProviderAccount(
  client: ApiClient,
  slug: string,
  input: RegisterGitProviderAccountInput
): Promise<GitProviderAccount> {
  const path = `/api/workspaces/${slug}/integrations/git/accounts/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(GitProviderAccount, envelope.data, `POST ${path}`);
}

/**
 * DELETE /api/workspaces/<slug>/integrations/git/accounts/<accountId>/ —
 * disconnect. The account stays listed as revoked, its bindings stay but
 * stop syncing. Answers `{ connected: false }`.
 */
export const GitAccountDisconnected = z.object({
  connected: z.boolean(),
});
export type GitAccountDisconnected = z.infer<typeof GitAccountDisconnected>;

export async function disconnectGitProviderAccount(
  client: ApiClient,
  slug: string,
  accountId: string
): Promise<GitAccountDisconnected> {
  const path = `/api/workspaces/${slug}/integrations/git/accounts/${accountId}/`;
  const envelope = await client.sendJson<unknown>("DELETE", path);
  return client.parse(GitAccountDisconnected, envelope.data, `DELETE ${path}`);
}

/** Verification timestamp helper shared by the status and account shapes. */
export function isVerified(status: GithubIntegrationStatus): boolean {
  return status.connected && typeof status.verified_at === "string" && status.verified_at.length > 0;
}
