// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pluggable runner-chat transport (NEWFRONT-549 RUN-032, messaging verbs
// NEWFRONT-551 RUN-027/029/030/031). The chat page never talks to a
// stream or to the chat endpoints directly: it reaches both through this
// seam, whose web default speaks the session's HTTP verbs and its
// server-sent event feed through `core/platform`, so a desktop build can
// replace the whole surface at bootstrap with native calls that deliver
// frames in the same shape. The page code does not change.

import {
  cancelChat,
  type CancelChatResult,
  createChatSession,
  ChatEvent,
  CHAT_EVENT_STREAM_NAME,
  chatEventsUrl,
  closeChat,
  getRunner,
  listChatMessages,
  listChatSessions,
  sendChatMessage,
  warmChatSession,
  type ApiClient,
  type ChatMessage,
  type ChatSession,
  type Runner,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { platform, type EventStream, type StreamInit } from "../../../core/platform/index.js";

/** One parsed frame from a session's event feed. */
export type ChatEventFrame = ChatEvent;

export interface ChatEventHandlers {
  /** Called once per well-formed frame, in arrival order. */
  onEvent: (event: ChatEventFrame) => void;
  /** An unreadable frame or a broken channel; the caller treats it as opaque. */
  onError?: (error: unknown) => void;
}

/** What a subscriber may want to know beyond the frames themselves. */
export interface ChatSubscriptionHandlers extends ChatEventHandlers {
  /**
   * The feed finished on its own (the server ended the response or the
   * channel dropped). The page may open a new subscription resuming past
   * the last frame it applied; an explicit unsubscribe never calls it.
   */
  onEnd?: () => void;
}

/**
 * The chat surface abstracted over its transport. `after` resumes past an
 * already-seen sequence (0 starts from the server's stored beginning).
 * The returned function unsubscribes and is safe to call twice.
 */
export interface ChatTransport {
  subscribeChatEvents(sessionId: string, after: number, handlers: ChatSubscriptionHandlers): () => void;
  /** Send the next user turn; answers the stored row. */
  sendChatMessage(sessionId: string, content: string): Promise<ChatMessage>;
  /** Interrupt the running turn, if any. */
  cancelChat(sessionId: string, reason?: string): Promise<CancelChatResult>;
  /** Close the conversation; answers the updated session. */
  closeChat(sessionId: string): Promise<ChatSession>;
  /**
   * Record the approval mode the engine should capture for this session's
   * next thread (RUN-035). Desktop transports keep it in memory; the
   * cloud path has no approval mode, so it is absent there.
   */
  setApprovalMode?(sessionId: string, mode: string): void;
  /** Answer one inline approval (RUN-034); desktop only. */
  decideChatApproval?(sessionId: string, localApprovalId: string, decision: string): Promise<void>;
  /**
   * Read and write verbs. The cloud implementations are `@pidash/api-client`
   * calls; a desktop transport replaces them with the local store and
   * daemon, which is why they are optional here.
   */
  chatRunner?(runnerId: string): Promise<Runner>;
  listChatSessions?(workspaceId: string, runnerId: string): Promise<ChatSession[]>;
  createChatSession?(input: { workspace: string; runner: string }): Promise<ChatSession>;
  listChatMessages?(sessionId: string): Promise<ChatMessage[]>;
  warmChatSession?(sessionId: string): Promise<void>;
}

/** Opens an event stream; injectable so the parser can be tested without a server. */
export type StreamOpener = (url: string, init: StreamInit) => EventStream;

const noop = (): void => undefined;

function parseFrame(data: string): ChatEventFrame | Error {
  let payload: unknown;
  try {
    payload = JSON.parse(data);
  } catch {
    return new Error("Unreadable chat frame");
  }
  const parsed = ChatEvent.safeParse(payload);
  return parsed.success ? parsed.data : new Error("Chat frame did not match its contract");
}

/** A source of the API client and of raw event streams, for tests and the desktop build. */
export interface CloudChatDeps {
  getClient?: () => ApiClient;
  openStream?: StreamOpener;
}

/**
 * The web default: a credentialed server-sent event feed for one session
 * and the session's own write verbs. Frames arrive under the named
 * `chat.event` type; other frame types are heartbeats and ignored.
 */
export function createCloudChatTransport(deps: CloudChatDeps = {}): ChatTransport {
  const getClient = deps.getClient ?? getAppClient;
  const openStream: StreamOpener = deps.openStream ?? ((url, init) => platform.stream(url, init));
  return {
    subscribeChatEvents(sessionId, after, { onEvent, onError = noop, onEnd = noop }) {
      const feed = openStream(chatEventsUrl(getClient(), sessionId, after), {
        headers: { Accept: "text/event-stream" },
      });
      let closed = false;
      const drain = async (): Promise<void> => {
        try {
          for await (const message of feed.messages()) {
            if (closed) return;
            if (message.event !== CHAT_EVENT_STREAM_NAME) continue;
            const frame = parseFrame(message.data);
            if (frame instanceof Error) {
              onError(frame);
              continue;
            }
            onEvent(frame);
          }
          if (!closed) onEnd();
        } catch (error) {
          if (!closed) {
            onError(error);
            onEnd();
          }
        }
      };
      void drain();
      return () => {
        if (closed) return;
        closed = true;
        feed.close();
      };
    },
    sendChatMessage: (sessionId, content) => sendChatMessage(getClient(), sessionId, content),
    cancelChat: (sessionId, reason) => cancelChat(getClient(), sessionId, reason),
    closeChat: (sessionId) => closeChat(getClient(), sessionId),
  };
}

const cloudTransport = createCloudChatTransport();

let activeTransport: ChatTransport = cloudTransport;

/** The transport the chat page subscribes through. */
export function getChatTransport(): ChatTransport {
  return activeTransport;
}

/** Replace the default (desktop bootstrap, tests); `null` restores the default. */
export function setChatTransport(transport: ChatTransport | null | undefined): void {
  activeTransport = transport ?? cloudTransport;
}

/** Runner detail for the chat header: local engine shim, else the API. */
export function fetchChatRunner(client: ApiClient, runnerId: string): Promise<Runner> {
  return activeTransport.chatRunner?.(runnerId) ?? getRunner(client, runnerId);
}

/** History list for one runner. */
export function fetchChatSessions(client: ApiClient, workspaceId: string, runnerId: string): Promise<ChatSession[]> {
  return (
    activeTransport.listChatSessions?.(workspaceId, runnerId) ??
    listChatSessions(client, { workspace: workspaceId, runner: runnerId })
  );
}

/** Open a conversation against the runner. */
export function openChatSession(client: ApiClient, input: { workspace: string; runner: string }): Promise<ChatSession> {
  return activeTransport.createChatSession?.(input) ?? createChatSession(client, input);
}

/** One conversation's saved transcript. */
export function fetchChatMessages(client: ApiClient, sessionId: string): Promise<ChatMessage[]> {
  return activeTransport.listChatMessages?.(sessionId) ?? listChatMessages(client, sessionId);
}

/**
 * Prepare a session so its first turn streams promptly (RUN-028). Local
 * transports warm the engine; the cloud path posts to the warm endpoint.
 */
export async function prepareChatSession(client: ApiClient, sessionId: string): Promise<void> {
  if (activeTransport.warmChatSession) {
    await activeTransport.warmChatSession(sessionId);
    return;
  }
  await warmChatSession(client, sessionId);
}
