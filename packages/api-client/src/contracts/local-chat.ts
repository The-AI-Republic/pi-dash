// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Local chat shapes: the TypeScript surface the desktop chat
// transport implements against the bundled daemon. Stored records,
// streamed daemon frames, the approval mode, and the transport
// interface live here; the host commands and event channels they
// describe are invoked by core/platform, never from this package.
import * as z from "zod/mini";
import type { ChatEvent, ChatMessage, ChatSession } from "./runner-chat.js";

/**
 * How eagerly the local engine asks before acting: prompt every
 * time, stay inside the working copy, or run with full access. Sent
 * on warm and send; the engine captures it when it spawns the
 * session's thread, so a change lands on the next thread, never on
 * a turn already in flight. Full access is the default, preserving
 * the engine's historical no-prompt posture when nothing was chosen.
 */
export const LocalApprovalMode = z.enum(["ask", "workspace", "full_access"]);
export type LocalApprovalMode = z.infer<typeof LocalApprovalMode>;

export const DEFAULT_LOCAL_APPROVAL_MODE: LocalApprovalMode = "full_access";

/**
 * A locally stored conversation. The runner id rides in `project`
 * (the store has no dedicated runner column); `engine_thread_id`
 * resumes the engine thread across restarts. Timestamps count
 * milliseconds since the epoch. A session touched since creation
 * has carried at least one message.
 */
export const LocalChatSessionRecord = z.object({
  id: z.string(),
  title: z.string(),
  workspace: z.string(),
  project: z.string(),
  working_dir: z.string(),
  engine_version: z.string(),
  engine_thread_id: z.nullable(z.string()),
  created_at: z.number(),
  updated_at: z.number(),
});
export type LocalChatSessionRecord = z.infer<typeof LocalChatSessionRecord>;

/**
 * A locally stored transcript row. `role` uses the store's own
 * vocabulary, which is wider than the UI's (approval rows read as
 * system upstairs). Tool calls and approval decisions persist as
 * encoded strings, null when the row carries neither.
 */
export const LocalChatEventRecord = z.object({
  id: z.string(),
  session_id: z.string(),
  seq: z.number(),
  role: z.string(),
  content: z.string(),
  tool_calls: z.nullable(z.string()),
  approval_decision: z.nullable(z.string()),
  created_at: z.number(),
});
export type LocalChatEventRecord = z.infer<typeof LocalChatEventRecord>;

/** Fields the store takes when opening a conversation. */
export const LocalChatNewSession = z.object({
  title: z.string(),
  workspace: z.string(),
  project: z.string(),
  engine_version: z.string(),
});
export type LocalChatNewSession = z.infer<typeof LocalChatNewSession>;

/** Fields the store takes when appending a transcript row. */
export const LocalChatAppendEvent = z.object({
  role: z.string(),
  content: z.string(),
});
export type LocalChatAppendEvent = z.infer<typeof LocalChatAppendEvent>;

// --- Streamed daemon frames -------------------------------------------------
// One tagged frame per engine milestone, in turn order. The transport
// translates these into the shared ChatEvent shape the chat page
// renders; only completed, failed and closed frames end a turn.

export const LocalChatFrameStarted = z.object({
  result: z.literal("chat_started"),
  data: z.object({
    chat_session_id: z.string(),
    local_thread_id: z.string(),
    local_session_id: z.optional(z.string()),
    started_at: z.string(),
  }),
});
export type LocalChatFrameStarted = z.infer<typeof LocalChatFrameStarted>;

export const LocalChatFrameMessageStarted = z.object({
  result: z.literal("chat_message_started"),
  data: z.object({
    chat_session_id: z.string(),
    message_id: z.string(),
    turn_id: z.optional(z.string()),
    started_at: z.string(),
  }),
});
export type LocalChatFrameMessageStarted = z.infer<typeof LocalChatFrameMessageStarted>;

export const LocalChatFrameEvent = z.object({
  result: z.literal("chat_event"),
  data: z.object({
    chat_session_id: z.string(),
    bridge_seq: z.number(),
    kind: z.string(),
    payload: z.record(z.string(), z.unknown()),
  }),
});
export type LocalChatFrameEvent = z.infer<typeof LocalChatFrameEvent>;

export const LocalChatFrameApproval = z.object({
  result: z.literal("chat_approval_request"),
  data: z.object({
    chat_session_id: z.string(),
    local_approval_id: z.string(),
    kind: z.string(),
    payload: z.record(z.string(), z.unknown()),
    reason: z.optional(z.string()),
    expires_at: z.optional(z.string()),
  }),
});
export type LocalChatFrameApproval = z.infer<typeof LocalChatFrameApproval>;

export const LocalChatFrameCompleted = z.object({
  result: z.literal("chat_message_completed"),
  data: z.object({
    chat_session_id: z.string(),
    message_id: z.string(),
    turn_id: z.optional(z.string()),
    assistant_message: z.optional(z.string()),
    status: z.string(),
    completed_at: z.string(),
  }),
});
export type LocalChatFrameCompleted = z.infer<typeof LocalChatFrameCompleted>;

export const LocalChatFrameFailed = z.object({
  result: z.literal("chat_failed"),
  data: z.object({
    chat_session_id: z.string(),
    code: z.string(),
    detail: z.optional(z.string()),
    failed_at: z.string(),
  }),
});
export type LocalChatFrameFailed = z.infer<typeof LocalChatFrameFailed>;

export const LocalChatFrameClosed = z.object({
  result: z.literal("chat_closed"),
  data: z.object({
    chat_session_id: z.string(),
    closed_at: z.string(),
  }),
});
export type LocalChatFrameClosed = z.infer<typeof LocalChatFrameClosed>;

export const LocalChatFrame = z.discriminatedUnion("result", [
  LocalChatFrameStarted,
  LocalChatFrameMessageStarted,
  LocalChatFrameEvent,
  LocalChatFrameApproval,
  LocalChatFrameCompleted,
  LocalChatFrameFailed,
  LocalChatFrameClosed,
]);
export type LocalChatFrame = z.infer<typeof LocalChatFrame>;

/** Transport-level failure for one session, outside any turn. */
export const LocalChatErrorEvent = z.object({
  chat_session_id: z.string(),
  message: z.string(),
});
export type LocalChatErrorEvent = z.infer<typeof LocalChatErrorEvent>;

/**
 * Answers the local engine takes for an inline approval. Unlike the
 * cloud endpoint this keeps the always-allow choice: commands may be
 * approved once for the whole session.
 */
export const LocalChatDecision = z.enum(["accept", "decline", "accept_for_session"]);
export type LocalChatDecision = z.infer<typeof LocalChatDecision>;

/** One translated event, delivered in arrival order. */
export type LocalChatEventHandler = (event: ChatEvent) => void;

/** Stream failure; the argument stays opaque to callers. */
export type LocalChatErrorHandler = (error: unknown) => void;

/** Tear a subscription down. Idempotent. */
export type LocalChatUnsubscribe = () => void;

/**
 * The chat surface over the bundled daemon, as the desktop slice
 * implements it. Session and transcript reads resolve from the local
 * store; turns run against the daemon with no cloud relay. Method
 * contracts (arguments and resolved shapes) mirror the cloud chat
 * contracts so the chat page stays transport-agnostic: stored rows
 * always read back open, without a runner echo or a closed state.
 */
export interface LocalChatTransport {
  listChatSessions(workspaceId: string, runnerId?: string): Promise<ChatSession[]>;
  createChatSession(input: { workspace: string; runner: string; model?: string; cwd?: string }): Promise<ChatSession>;
  listChatMessages(sessionId: string): Promise<ChatMessage[]>;
  sendChatMessage(sessionId: string, content: string): Promise<ChatMessage>;
  warmChatSession(sessionId: string): Promise<{ ok: boolean; skipped?: string }>;
  cancelChat(sessionId: string, reason?: string): Promise<{ ok: boolean }>;
  closeChat(sessionId: string): Promise<ChatSession>;
  /**
   * Answer a pending inline approval. Local-only: the cloud path
   * decides approvals through its own endpoint instead.
   */
  decideChatApproval(sessionId: string, localApprovalId: string, decision: LocalChatDecision): Promise<void>;
  /**
   * Subscribe to a session's translated events. `after` is accepted
   * for interface symmetry but ignored: history backfills from the
   * local store, so there is nothing to resume past.
   */
  subscribeChatEvents(
    sessionId: string,
    after: number,
    onEvent: LocalChatEventHandler,
    onError?: LocalChatErrorHandler
  ): LocalChatUnsubscribe;
  /** Approval mode the session's next thread warms under. */
  setApprovalMode(sessionId: string, mode: LocalApprovalMode): void;
  getApprovalMode(sessionId: string): LocalApprovalMode;
}
