// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop engine endpoints: the model profile the bundled engine
// should call, and the short-lived credential to call it with. Web
// chat never calls these — only desktop-gated sessions may — but the
// shapes are contracted here so the desktop path typechecks.
import * as z from "zod/mini";
import { ApiClient, CSRF_HEADER } from "../client.js";
import { isApiError } from "../errors.js";
import { dateTime } from "./fields.js";
import { AssistantApiError } from "./assistant-errors.js";

/**
 * The engine profile: which lane to call and whether it may run.
 * Never carries a credential — the profile is safe to poll, and the
 * operator kill switch folds into `available` without an app update.
 */
export const AgentProfile = z.object({
  managed_runner_enabled: z.boolean(),
  lane: z.string(),
  base_url: z.string(),
  model: z.string(),
  available: z.boolean(),
  reason_code: z.string(),
  graceful_stop_seconds: z.number(),
});
export type AgentProfile = z.infer<typeof AgentProfile>;

/** Read the engine profile (desktop sessions only). */
export async function getAgentProfile(client: ApiClient): Promise<AgentProfile> {
  const body = await client.getJson<unknown>("/api/users/me/ai-assistant/agent-profile/");
  return client.parse(AgentProfile, body, "GET /api/users/me/ai-assistant/agent-profile/");
}

/** The minted credential: the token plus its expiry. */
export const AgentToken = z.object({
  token: z.string(),
  expires_at: dateTime(),
});
export type AgentToken = z.infer<typeof AgentToken>;

/** Options for the credential mint. */
export interface PostAgentTokenOptions {
  /**
   * A CSRF token read elsewhere, e.g. the cloud path on a deployment
   * whose web server proxies only `/api/*` (the desktop agent's reason).
   * Supplied, it replaces the client's default token read.
   */
  csrfToken?: string;
}

/**
 * Mint a short-lived engine credential with an empty-body POST. The
 * desktop re-fetches before expiry; nothing persists server-side.
 */
export async function postAgentToken(client: ApiClient, options: PostAgentTokenOptions = {}): Promise<AgentToken> {
  const path = "/api/users/me/ai-assistant/agent-token/";
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: {},
    ...(options.csrfToken === undefined ? {} : { headers: { [CSRF_HEADER]: options.csrfToken } }),
  });
  return client.parse(AgentToken, envelope.data, `POST ${path}`);
}

/**
 * Read the desktop-gate refusal off a rejected call: a browser
 * session meets 403 naming `desktop_session_required`, which tells
 * "not for browsers" apart from "not signed in". Null otherwise.
 */
export function desktopSessionRequiredFrom(error: unknown): { detail: string } | null {
  if (!isApiError(error) || error.status !== 403) return null;
  const parsed = AssistantApiError.safeParse(error.body);
  if (!parsed.success || parsed.data.error !== "desktop_session_required") return null;
  return { detail: parsed.data.detail ?? "" };
}
