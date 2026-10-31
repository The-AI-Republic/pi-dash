// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner chat contracts: conversation sessions against one runner, the
// message history inside a session, the warm/send/cancel/close verbs,
// the resumable server-sent event feed, and the decide flow for
// chat-originated approvals. Desktop local chat speaks the same shapes
// through its own transport (see local-chat.ts); only the cloud HTTP
// path lives here.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";

export const ChatSessionStatus = z.enum(["open", "closed", "failed"]);
export type ChatSessionStatus = z.infer<typeof ChatSessionStatus>;

export const ChatMessageRole = z.enum(["user", "assistant", "tool", "system"]);
export type ChatMessageRole = z.infer<typeof ChatMessageRole>;

export const ChatMessageStatus = z.enum(["queued", "sent", "streaming", "completed", "failed", "cancelled"]);
export type ChatMessageStatus = z.infer<typeof ChatMessageStatus>;

export const ChatApprovalKind = z.enum(["command_execution", "file_change", "network_access", "other"]);
export type ChatApprovalKind = z.infer<typeof ChatApprovalKind>;

export const ChatApprovalStatus = z.enum(["pending", "accepted", "declined", "expired"]);
export type ChatApprovalStatus = z.infer<typeof ChatApprovalStatus>;

/**
 * Decisions the cloud chat-approval endpoint takes. Only the one-shot
 * pair is accepted here: the always-allow choice the desktop offers is
 * a local-engine behavior, and sending it to this endpoint is rejected.
 */
export const ChatApprovalDecision = z.enum(["accept", "decline"]);
export type ChatApprovalDecision = z.infer<typeof ChatApprovalDecision>;

/**
 * The runner nested inside a chat session echo. A structural subset on
 * purpose: identity plus the presence the chat header reads. The full
 * runner representation (pod and machine detail, live state) is owned
 * by the runners contracts; parsing strips the fields not declared
 * here, so this stays green as that shape grows.
 */
export const ChatSessionRunner = z.object({
  id: uuid(),
  name: z.string(),
  /** Presence word: one of online, busy, offline, revoked. */
  status: z.string(),
});
export type ChatSessionRunner = z.infer<typeof ChatSessionRunner>;

/**
 * One conversation against a runner. `runner_detail` is always present
 * on the cloud path (the runner reference is required); the local path
 * leaves it empty. `cwd` echoes blank — the cloud ignores a
 * caller-chosen directory and the runner resolves its own. `model` is
 * the stored override truncated server-side, blank for the default.
 * `last_message_at` orders the history list and is null until the
 * first turn; `active_message_id`/`active_turn_id` name the turn in
 * flight, if any.
 */
export const ChatSession = z.object({
  id: uuid(),
  workspace: uuid(),
  runner: uuid(),
  runner_detail: ChatSessionRunner,
  created_by: uuid(),
  pod: uuid(),
  status: ChatSessionStatus,
  agent_kind: z.string(),
  local_thread_id: z.string(),
  local_session_id: z.string(),
  cwd: z.string(),
  model: z.string(),
  active_turn_id: z.string(),
  active_message_id: nullableUuid(),
  close_requested: z.boolean(),
  last_message_at: nullableDateTime(),
  closed_at: nullableDateTime(),
  error: z.string(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type ChatSession = z.infer<typeof ChatSession>;

export const ChatSessionList = z.array(ChatSession);
export type ChatSessionList = z.infer<typeof ChatSessionList>;

export interface ChatSessionQuery {
  workspace?: string;
  runner?: string;
  project?: string;
}

/**
 * List sessions, newest activity first, capped server-side. Every
 * filter is optional: without a workspace the view answers the
 * caller's own sessions plus whatever their admin scope can see, and
 * an unknown runner id answers an empty list rather than an error.
 */
export async function listChatSessions(client: ApiClient, query: ChatSessionQuery): Promise<ChatSessionList> {
  const path = "/api/runners/chat/sessions/";
  const body = await client.getJson<unknown>(path, {
    query: { workspace: query.workspace, runner: query.runner, project: query.project },
  });
  return client.parse(ChatSessionList, body, `GET ${path}`);
}

export interface ChatSessionCreate {
  workspace: string;
  runner: string;
  model?: string;
  cwd?: string;
}

/**
 * Open a session against a runner. Creating answers 201, except when
 * the caller already owns an empty open session on that runner, which
 * is handed back instead. Offline and revoked runners are refused, as
 * are runners outside the caller's reach (reported as not found).
 */
export async function createChatSession(client: ApiClient, input: ChatSessionCreate): Promise<ChatSession> {
  const path = "/api/runners/chat/sessions/";
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(ChatSession, envelope.data, `POST ${path}`);
}

/** Read one session back by id. Foreign sessions read as not found. */
export async function getChatSession(client: ApiClient, sessionId: string): Promise<ChatSession> {
  const path = `/api/runners/chat/sessions/${sessionId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(ChatSession, body, `GET ${path}`);
}

/**
 * One transcript row. `content_parts` carries structured attachments
 * (empty for plain turns); `seq` orders the transcript from 1;
 * `completed_at` is set once the turn finishes. The local ids link a
 * row to the engine-side turn that produced it.
 */
export const ChatMessage = z.object({
  id: uuid(),
  session: uuid(),
  role: ChatMessageRole,
  content: z.string(),
  content_parts: z.array(z.unknown()),
  status: ChatMessageStatus,
  local_item_id: z.string(),
  local_turn_id: z.string(),
  seq: z.number(),
  created_at: dateTime(),
  completed_at: nullableDateTime(),
});
export type ChatMessage = z.infer<typeof ChatMessage>;

export const ChatMessageList = z.array(ChatMessage);
export type ChatMessageList = z.infer<typeof ChatMessageList>;

/** Transcript of a session in sequence order, oldest first. */
export async function listChatMessages(client: ApiClient, sessionId: string): Promise<ChatMessageList> {
  const path = `/api/runners/chat/sessions/${sessionId}/messages/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(ChatMessageList, body, `GET ${path}`);
}

/**
 * Send the next user turn. Answers the stored user row; the reply
 * arrives over the event feed. Blank drafts are refused, as are sends
 * to a closed session, to an unreachable runner, or while a turn is
 * already in flight. Oversized payloads are refused above the
 * server-side byte cap, and the verb is rate-limited per user.
 */
export async function sendChatMessage(client: ApiClient, sessionId: string, content: string): Promise<ChatMessage> {
  const path = `/api/runners/chat/sessions/${sessionId}/messages/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { content, content_parts: [] } });
  return client.parse(ChatMessage, envelope.data, `POST ${path}`);
}

export const WarmChatResult = z.object({
  ok: z.boolean(),
  skipped: z.optional(z.string()),
});
export type WarmChatResult = z.infer<typeof WarmChatResult>;

/**
 * Prepare a session so the first turn streams promptly. Warming while
 * a turn runs is a no-op that names the skip reason; warming a closed
 * session or an unreachable runner is refused.
 */
export async function warmChatSession(client: ApiClient, sessionId: string): Promise<WarmChatResult> {
  const path = `/api/runners/chat/sessions/${sessionId}/warm/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: {} });
  return client.parse(WarmChatResult, envelope.data, `POST ${path}`);
}

export const CancelChatResult = z.object({
  ok: z.boolean(),
  noop: z.optional(z.boolean()),
});
export type CancelChatResult = z.infer<typeof CancelChatResult>;

/**
 * Stop the turn in flight, if any. Cancelling an idle session still
 * answers ok and flags the call as a no-op. The reason defaults to
 * the user-cancelled marker the runner reports back.
 */
export async function cancelChat(client: ApiClient, sessionId: string, reason?: string): Promise<CancelChatResult> {
  const path = `/api/runners/chat/sessions/${sessionId}/cancel/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { reason: reason ?? "user_cancelled" } });
  return client.parse(CancelChatResult, envelope.data, `POST ${path}`);
}

/**
 * Close a session. Closing mid-turn parks the close and stops the
 * turn first; otherwise the session moves to closed immediately and
 * the updated row is answered.
 */
export async function closeChat(client: ApiClient, sessionId: string): Promise<ChatSession> {
  const path = `/api/runners/chat/sessions/${sessionId}/close/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: {} });
  return client.parse(ChatSession, envelope.data, `POST ${path}`);
}

/**
 * One streamed event. `seq` is the per-session sequence the stream
 * resumes by; `message` links deltas to their transcript row (null for
 * session-level frames); `kind` names the frame and `payload` carries
 * it opaquely, since engines add kinds without a server change.
 */
export const ChatEvent = z.object({
  id: z.number(),
  session: uuid(),
  message: nullableUuid(),
  seq: z.number(),
  kind: z.string(),
  payload: z.record(z.string(), z.unknown()),
  created_at: dateTime(),
});
export type ChatEvent = z.infer<typeof ChatEvent>;

/** Named event the stream delivers frames on. */
export const CHAT_EVENT_STREAM_NAME = "chat.event";

/**
 * Absolute URL of a session's server-sent event feed. `after` resumes
 * past an already-seen sequence (omitted for a fresh subscription);
 * the server also honors the Last-Event-ID header, and replays
 * whatever is stored before following the live channel. The stream
 * stays open with periodic heartbeat comments until unsubscribed.
 */
export function chatEventsUrl(client: ApiClient, sessionId: string, after?: number): string {
  const url = `${client.baseUrl}/api/runners/chat/sessions/${sessionId}/events/`;
  return after !== undefined && after > 0 ? `${url}?after=${after}` : url;
}

/**
 * One pending approval raised by a chat turn. `payload` carries the
 * request opaquely (command, path, or address, depending on kind);
 * `reason` is the engine's own explanation; `expires_at` bounds the
 * prompt's life. `decided_by` is set once answered.
 */
export const ChatApprovalRequest = z.object({
  id: uuid(),
  session: uuid(),
  local_approval_id: z.string(),
  kind: ChatApprovalKind,
  payload: z.record(z.string(), z.unknown()),
  reason: z.string(),
  status: ChatApprovalStatus,
  decision_source: z.string(),
  decided_by: nullableUuid(),
  requested_at: dateTime(),
  expires_at: nullableDateTime(),
  decided_at: nullableDateTime(),
});
export type ChatApprovalRequest = z.infer<typeof ChatApprovalRequest>;

export const ChatApprovalList = z.array(ChatApprovalRequest);
export type ChatApprovalList = z.infer<typeof ChatApprovalList>;

/**
 * Pending chat-originated approvals, newest first, capped
 * server-side. Without a workspace the view answers the caller's own
 * sessions plus whatever their admin scope can see.
 */
export async function listChatApprovals(client: ApiClient, workspace?: string): Promise<ChatApprovalList> {
  const path = "/api/runners/chat/approvals/";
  const body = await client.getJson<unknown>(path, { query: { workspace } });
  return client.parse(ChatApprovalList, body, `GET ${path}`);
}

/**
 * Answer a pending chat approval. The decided row is answered with
 * the decider and the web decision source stamped. Answering twice is
 * refused, as is answering outside the caller's reach.
 */
export async function decideChatApproval(
  client: ApiClient,
  approvalId: string,
  decision: ChatApprovalDecision
): Promise<ChatApprovalRequest> {
  const path = `/api/runners/chat/approvals/${approvalId}/decide/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { decision } });
  return client.parse(ChatApprovalRequest, envelope.data, `POST ${path}`);
}
