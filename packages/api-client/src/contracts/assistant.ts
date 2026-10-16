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
 */
export const LlmConfig = z.object({
  provider_kind: z.string(),
  base_url: z.string(),
  model_name: z.string(),
  has_api_key: z.boolean(),
  last_verified_at: nullableDateTime(),
});
export type LlmConfig = z.infer<typeof LlmConfig>;

export async function getLlmConfig(client: ApiClient): Promise<LlmConfig> {
  const body = await client.getJson<unknown>("/api/users/me/ai-assistant/config/");
  return client.parse(LlmConfig, body, "GET /api/users/me/ai-assistant/config/");
}
