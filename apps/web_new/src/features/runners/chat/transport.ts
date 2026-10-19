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
  ChatEvent,
  CHAT_EVENT_STREAM_NAME,
  chatEventsUrl,
  closeChat,
  sendChatMessage,
  type ApiClient,
  type ChatMessage,
  type ChatSession,
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
   * Hand the runner's approval mode to the conversation before a turn.
   * Desktop transports implement this for their local engine; the cloud
   * path has no approval mode, so it is absent there.
   */
  applyApprovalMode?(sessionId: string): void;
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
