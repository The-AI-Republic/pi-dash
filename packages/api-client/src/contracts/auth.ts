// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Session auth contracts: sign-out, deactivation, and the instance
// config the session bootstrap reads. The signed-out entry flows
// (sign-in, sign-up, recovery, device, bootstrap) live in
// auth-entry.ts, out of the initial bundle (NEWFRONT-523, size gate).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";

export interface SignOutResult {
  location: string;
}

export async function signOut(client: ApiClient): Promise<SignOutResult> {
  const response = await client.post("/auth/sign-out/", {});
  // The session is gone; its CSRF token goes with it.
  client.clearCsrfCache();
  return { location: response.url };
}

export async function deactivateAccount(client: ApiClient): Promise<number> {
  const response = await client.delete("/api/users/me/");
  // Deactivation wipes every session server-side; the CSRF token goes with them.
  client.clearCsrfCache();
  return response.status;
}

/**
 * Instance flags the auth cards read: sign-up and workspace-creation
 * gates, enabled providers, the email login methods, the mail posture,
 * and the self-managed marker — plus the configured changelog page the
 * help menu's updates dialog embeds (empty when unconfigured), and the
 * file-size ceiling in bytes the attachment dropzones enforce (the
 * server default is five megabytes; readers fall back to it when the
 * key is absent). The payload carries more; the areas read these.
 */
export const InstanceConfig = z.object({
  enable_signup: z.boolean(),
  is_workspace_creation_disabled: z.boolean(),
  is_google_enabled: z.boolean(),
  is_github_enabled: z.boolean(),
  is_gitlab_enabled: z.boolean(),
  is_gitea_enabled: z.boolean(),
  is_magic_login_enabled: z.boolean(),
  is_email_password_enabled: z.boolean(),
  is_smtp_configured: z.boolean(),
  is_self_managed: z.boolean(),
  instance_changelog_url: z.string(),
  file_size_limit: z.optional(z.number()),
  /**
   * Whether the instance holds an LLM key for the editor writing helper
   * (AGT-063). Absent on backends older than the flag, which reads as
   * unconfigured.
   */
  has_llm_configured: z.optional(z.boolean()),
});
export type InstanceConfig = z.infer<typeof InstanceConfig>;

const InstanceResponse = z.object({
  config: InstanceConfig,
});

export async function getInstanceConfig(client: ApiClient): Promise<InstanceConfig> {
  const body = await client.getJson<unknown>("/api/instances/");
  return client.parse(InstanceResponse, body, "GET /api/instances/").config;
}
