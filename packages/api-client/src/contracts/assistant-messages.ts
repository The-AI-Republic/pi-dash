// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Thread writes, the transcript read, the send, the stop and the
// title helper: the workspace-scoped assistant endpoints behind the
// thread view. There is intentionally no thread GET — the detail
// route only answers PATCH and DELETE, and the view reads the list.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { AssistantThread } from "./assistant.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";

/** Transcript roles the server persists (notice is client-synthesized). */
export const AssistantMessageRole = z.enum(["user", "assistant", "tool_call", "tool_result", "error"]);
export type AssistantMessageRole = z.infer<typeof AssistantMessageRole>;

export const AssistantMessageStatus = z.enum(["streaming", "completed", "failed", "cancelled"]);
export type AssistantMessageStatus = z.infer<typeof AssistantMessageStatus>;

/**
 * Deep link a tool-activity row carries (write actions stay visible). Only
 * the address is guaranteed: issue links add their identifiers, and the
 * label is the row's own display text when the server sent one (AGT-046).
 */
export const AssistantMessageLink = z.object({
  type: z.optional(z.string()),
  workspace_slug: z.optional(z.string()),
  project_id: z.optional(z.string()),
  issue_id: z.optional(z.string()),
  url_path: z.string(),
  label: z.optional(z.string()),
});
export type AssistantMessageLink = z.infer<typeof AssistantMessageLink>;

/**
 * One transcript row. The list endpoint and the event stream emit the
 * same shape; anything beyond the declared links is stripped.
 */
export const AssistantMessage = z.object({
  id: uuid(),
  role: AssistantMessageRole,
  content: z.string(),
  status: AssistantMessageStatus,
  seq: z.number(),
  turn_id: nullableUuid(),
  payload: z.object({
    links: z.optional(z.array(AssistantMessageLink)),
  }),
  created_at: dateTime(),
  completed_at: nullableDateTime(),
});
export type AssistantMessage = z.infer<typeof AssistantMessage>;

export const AssistantMessageList = z.array(AssistantMessage);
export type AssistantMessageList = z.infer<typeof AssistantMessageList>;

export const AssistantTurnStatus = z.enum(["queued", "running", "completed", "failed", "cancelled"]);
export type AssistantTurnStatus = z.infer<typeof AssistantTurnStatus>;

/** The 202 answer to a send: the queued turn plus the stored user row. */
export const AssistantSendResponse = z.object({
  turn: z.object({
    id: uuid(),
    status: AssistantTurnStatus,
  }),
  message: AssistantMessage,
});
export type AssistantSendResponse = z.infer<typeof AssistantSendResponse>;

export interface AssistantThreadPatch {
  title?: string;
  is_archived?: boolean;
}

/**
 * Rename or (un)archive a thread. Answers the thread; the chat surface
 * keeps no buttons for this — the methods stay reserved for later.
 */
export async function updateAssistantThread(
  client: ApiClient,
  slug: string,
  threadId: string,
  patch: AssistantThreadPatch
): Promise<AssistantThread> {
  const path = `/api/workspaces/${slug}/ai-assistant/threads/${threadId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, {
    json: { title: patch.title, is_archived: patch.is_archived },
  });
  return client.parse(AssistantThread, envelope.data, `PATCH ${path}`);
}

/** Delete a thread; resolves on the 204 with nothing to parse. */
export async function deleteAssistantThread(client: ApiClient, slug: string, threadId: string): Promise<void> {
  await client.delete(`/api/workspaces/${slug}/ai-assistant/threads/${threadId}/`);
}

export interface AssistantMessageQuery {
  /** Transcript cursor: only rows sequenced after this one. */
  after?: number;
  /** Page size; the server clamps it into 1..200. */
  limit?: number;
}

/**
 * One transcript page, oldest first. The cursor is the last seen
 * sequence number, so paging and the live stream merge by seq.
 */
export async function listAssistantMessages(
  client: ApiClient,
  slug: string,
  threadId: string,
  query: AssistantMessageQuery = {}
): Promise<AssistantMessageList> {
  const path = `/api/workspaces/${slug}/ai-assistant/threads/${threadId}/messages/`;
  const body = await client.getJson<unknown>(path, { query: { after: query.after, limit: query.limit } });
  return client.parse(AssistantMessageList, body, `GET ${path}`);
}

/**
 * Send one message: stores the user row, queues the turn, titles an
 * untitled thread from the first line, and answers 202. Refusals
 * (empty, too long, no key, turn active, thread full) reject with
 * the assistant failure shape — see assistant-errors.
 */
export async function sendAssistantMessage(
  client: ApiClient,
  slug: string,
  threadId: string,
  content: string
): Promise<AssistantSendResponse> {
  const path = `/api/workspaces/${slug}/ai-assistant/threads/${threadId}/messages/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { content } });
  return client.parse(AssistantSendResponse, envelope.data, `POST ${path}`);
}

/**
 * Stop the running turn; resolves on the 204. With no active turn
 * the server refuses (`no_active_turn`) instead of no-op succeeding.
 */
export async function cancelAssistantTurn(client: ApiClient, slug: string, threadId: string): Promise<void> {
  await client.post(`/api/workspaces/${slug}/ai-assistant/threads/${threadId}/cancel/`);
}

/** The title helper answer: the generated title, nothing else. */
export const AssistantGeneratedTitle = z.object({
  title: z.string(),
});
export type AssistantGeneratedTitle = z.infer<typeof AssistantGeneratedTitle>;

/**
 * Title the create-issue dialog's draft from its description. Chat
 * titles come from the send path instead — this serves that dialog.
 */
export async function generateAssistantTitle(
  client: ApiClient,
  slug: string,
  description: string
): Promise<AssistantGeneratedTitle> {
  const path = `/api/workspaces/${slug}/ai-assistant/generate-title/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { description } });
  return client.parse(AssistantGeneratedTitle, envelope.data, `POST ${path}`);
}
