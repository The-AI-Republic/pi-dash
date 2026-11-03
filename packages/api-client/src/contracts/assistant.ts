// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant contracts: the thread list behind the home assistant card and
// the per-user model-config check that decides whether the assistant can
// run (unconfigured when no model name or key is set).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, uuid } from "./fields.js";

export const AssistantThread = z.object({
  id: uuid(),
  title: z.string(),
  is_archived: z.boolean(),
  has_active_turn: z.boolean(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type AssistantThread = z.infer<typeof AssistantThread>;

export const AssistantThreadList = z.array(AssistantThread);
export type AssistantThreadList = z.infer<typeof AssistantThreadList>;

/**
 * Chat threads for the workspace, newest first, capped server-side.
 * Loop (automation) threads never surface here.
 */
export async function listAssistantThreads(client: ApiClient, slug: string): Promise<AssistantThreadList> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/ai-assistant/threads/`);
  return client.parse(AssistantThreadList, body, `GET /api/workspaces/${slug}/ai-assistant/threads/`);
}

export async function createAssistantThread(client: ApiClient, slug: string, title = ""): Promise<AssistantThread> {
  const envelope = await client.sendJson<unknown>("POST", `/api/workspaces/${slug}/ai-assistant/threads/`, {
    json: { title },
  });
  return client.parse(AssistantThread, envelope.data, `POST /api/workspaces/${slug}/ai-assistant/threads/`);
}

/**
 * The user's model configuration. Absent configuration reads back as the
 * blank default (empty model name, no key) rather than 404.
 *
 * The cloud backend adds the credential-lane field plus one credential
 * flag per lane; older backends omit them all, and the lane hook falls
 * back to the self-supplied lane (see resolveCredentialLane).
 */
export const CREDENTIAL_LANES = ["byok", "codex", "openhub"] as const;
export type CredentialLane = (typeof CREDENTIAL_LANES)[number];

export const LlmConfig = z.object({
  provider_kind: z.string(),
  base_url: z.string(),
  model_name: z.string(),
  has_api_key: z.boolean(),
  last_verified_at: nullableDateTime(),
  credential_mode: z.optional(z.enum(CREDENTIAL_LANES)),
  has_byok_api_key: z.optional(z.boolean()),
  has_codex_token: z.optional(z.boolean()),
  has_openhub_access: z.optional(z.boolean()),
  has_openai_api_key: z.optional(z.boolean()),
  codex_model_name: z.optional(z.string()),
  codex_model_options: z.optional(z.array(z.string())),
  codex_last_verified_at: z.optional(nullableDateTime()),
});
export type LlmConfig = z.infer<typeof LlmConfig>;

export async function getLlmConfig(client: ApiClient): Promise<LlmConfig> {
  const body = await client.getJson<unknown>("/api/users/me/ai-assistant/config/");
  return client.parse(LlmConfig, body, "GET /api/users/me/ai-assistant/config/");
}

/**
 * The active credential lane behind the provider config: self-supplied
 * keys, the provider account, or the hosted lane. Older backends predate
 * the lane field and read as the self-supplied lane.
 */
export function resolveCredentialLane(config: LlmConfig): CredentialLane {
  return config.credential_mode ?? "byok";
}

/**
 * The active-config test answer. Success stamps the connection verified
 * server-side; failures carry a machine code plus human detail.
 */
export const LlmConfigTestResult = z.object({
  ok: z.boolean(),
  error_code: z.optional(z.string()),
  detail: z.optional(z.string()),
});
export type LlmConfigTestResult = z.infer<typeof LlmConfigTestResult>;

export async function testLlmConfig(client: ApiClient): Promise<LlmConfigTestResult> {
  const envelope = await client.sendJson<unknown>("POST", "/api/users/me/ai-assistant/config/test/", {
    json: {},
  });
  return client.parse(LlmConfigTestResult, envelope.data, "POST /api/users/me/ai-assistant/config/test/");
}

/**
 * Connection kinds: self-supplied keys, the permanent provider-account
 * row, and the hosted lane (which carries no credential of its own).
 */
export const ASSISTANT_CONNECTION_KINDS = ["byok", "codex", "openhub"] as const;
export type AssistantConnectionKind = (typeof ASSISTANT_CONNECTION_KINDS)[number];

/**
 * One saved provider connection. Single-row answers (create, select,
 * model update) carry exactly this; list rows add the connected and
 * plan-usage flags (see AssistantConnectionListItem).
 */
export const AssistantConnection = z.object({
  id: uuid(),
  kind: z.enum(ASSISTANT_CONNECTION_KINDS),
  provider_kind: z.string(),
  base_url: z.string(),
  model_name: z.string(),
  has_api_key: z.boolean(),
  is_selected: z.boolean(),
  last_verified_at: nullableDateTime(),
  created_at: nullableDateTime(),
});
export type AssistantConnection = z.infer<typeof AssistantConnection>;

/**
 * A list row: the stored connection plus the live flags the list view
 * annotates — whether the row is usable right now, and whether the
 * provider-account row draws on its plan.
 */
export const AssistantConnectionListItem = z.extend(AssistantConnection, {
  connected: z.boolean(),
  uses_chatgpt_plan: z.boolean(),
});
export type AssistantConnectionListItem = z.infer<typeof AssistantConnectionListItem>;

/**
 * The connections list: rows plus the managed-model options, the
 * provider sign-in availability, and the hosted-lane availability flag.
 * Older backends predate the lane flag; resolveOpenhubAvailable reads
 * a missing flag as available so they never banner every row.
 */
export const AssistantConnectionList = z.object({
  connections: z.array(AssistantConnectionListItem),
  codex_model_options: z.array(z.string()),
  codex_model_labels: z.record(z.string(), z.string()),
  has_codex_credential: z.boolean(),
  openai_login_available: z.boolean(),
  openhub_model_options: z.array(z.string()),
  openhub_available: z.optional(z.boolean()),
  openhub_wallet_url: z.string(),
});
export type AssistantConnectionList = z.infer<typeof AssistantConnectionList>;

/**
 * Whether the hosted lane is available on this backend. Older backends
 * predate the lane flag entirely, so a missing flag reads as available
 * rather than banner-every-row.
 */
export function resolveOpenhubAvailable(list: AssistantConnectionList): boolean {
  return list.openhub_available ?? true;
}

export async function listAssistantConnections(client: ApiClient): Promise<AssistantConnectionList> {
  const body = await client.getJson<unknown>("/api/users/me/ai-assistant/connections/");
  return client.parse(AssistantConnectionList, body, "GET /api/users/me/ai-assistant/connections/");
}

export interface AssistantConnectionCreateInput {
  providerKind: string;
  baseUrl?: string;
  modelName: string;
  apiKey: string;
}

/**
 * Add a self-supplied-key connection. The provider-account row is added
 * by provider sign-in instead, never here. The new row selects itself.
 */
export async function createAssistantConnection(
  client: ApiClient,
  input: AssistantConnectionCreateInput
): Promise<AssistantConnection> {
  const envelope = await client.sendJson<unknown>("POST", "/api/users/me/ai-assistant/connections/", {
    json: {
      provider_kind: input.providerKind,
      base_url: input.baseUrl,
      model_name: input.modelName,
      api_key: input.apiKey,
    },
  });
  return client.parse(AssistantConnection, envelope.data, "POST /api/users/me/ai-assistant/connections/");
}

/**
 * Rename a connection's model. Managed rows only accept models from the
 * account's discovered catalog; a saved choice stays valid until the
 * user reconnects and refreshes it.
 */
export async function updateConnectionModel(
  client: ApiClient,
  connectionId: string,
  modelName: string
): Promise<AssistantConnection> {
  const envelope = await client.sendJson<unknown>(
    "PATCH",
    `/api/users/me/ai-assistant/connections/${encodeURIComponent(connectionId)}/`,
    { json: { model_name: modelName } }
  );
  return client.parse(
    AssistantConnection,
    envelope.data,
    `PATCH /api/users/me/ai-assistant/connections/${connectionId}/`
  );
}

/**
 * Remove a self-supplied connection. Answers 204; removing the selected
 * row falls back to the remaining connected row. The provider-account
 * row is permanent — disconnect it instead.
 */
export async function deleteAssistantConnection(client: ApiClient, connectionId: string): Promise<void> {
  await client.delete(`/api/users/me/ai-assistant/connections/${encodeURIComponent(connectionId)}/`);
}

/** Switch the active connection. The row it answers carries the new state. */
export async function selectAssistantConnection(client: ApiClient, connectionId: string): Promise<AssistantConnection> {
  const envelope = await client.sendJson<unknown>(
    "POST",
    `/api/users/me/ai-assistant/connections/${encodeURIComponent(connectionId)}/select/`,
    { json: {} }
  );
  return client.parse(
    AssistantConnection,
    envelope.data,
    `POST /api/users/me/ai-assistant/connections/${connectionId}/select/`
  );
}

/**
 * Disconnect the provider account. Answers 204; the row stays in the
 * list, unusable until an account backs it again.
 */
export async function disconnectAssistantConnection(client: ApiClient, connectionId: string): Promise<void> {
  await client.post(`/api/users/me/ai-assistant/connections/${encodeURIComponent(connectionId)}/disconnect/`, {
    json: {},
  });
}

/**
 * The provider-login start answer: the authorize address the popup
 * navigates to, plus the state the status poll carries back.
 */
export const OpenAILoginStart = z.object({
  authorize_url: z.string(),
  state: z.string(),
});
export type OpenAILoginStart = z.infer<typeof OpenAILoginStart>;

export async function startOpenAILogin(client: ApiClient): Promise<OpenAILoginStart> {
  const envelope = await client.sendJson<unknown>("POST", "/api/users/me/ai-assistant/openai-login/", {
    json: {},
  });
  return client.parse(OpenAILoginStart, envelope.data, "POST /api/users/me/ai-assistant/openai-login/");
}

/** Provider-login poll states: pending, done, provider-side failure, gone. */
export const OPENAI_LOGIN_STATUSES = ["pending", "ok", "error", "expired"] as const;
export type OpenAILoginStatusValue = (typeof OPENAI_LOGIN_STATUSES)[number];

export const OpenAILoginStatus = z.object({
  status: z.enum(OPENAI_LOGIN_STATUSES),
  detail: z.optional(z.string()),
});
export type OpenAILoginStatus = z.infer<typeof OpenAILoginStatus>;

export async function getOpenAILoginStatus(client: ApiClient, state: string): Promise<OpenAILoginStatus> {
  const body = await client.getJson<unknown>("/api/users/me/ai-assistant/openai-login/status/", {
    query: { state },
  });
  return client.parse(OpenAILoginStatus, body, "GET /api/users/me/ai-assistant/openai-login/status/");
}
