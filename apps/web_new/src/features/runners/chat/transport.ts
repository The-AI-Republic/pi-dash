// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pluggable runner-chat event transport (NEWFRONT-549, RUN-032). The chat
// page never talks to a stream directly: it subscribes through this seam,
// whose web default reads the session's server-sent event feed through
// `core/platform` so the desktop build relays frames the same way. A
// desktop transport can replace the default at bootstrap with
// `setChatTransport`; the page code does not change.

import { ChatEvent, CHAT_EVENT_STREAM_NAME, chatEventsUrl, type ApiClient } from "@pidash/api-client";

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

/**
 * The chat surface abstracted over its stream. `after` resumes past an
 * already-seen sequence (0 starts from the server's stored beginning).
 * The returned function unsubscribes and is safe to call twice.
 */
export interface ChatTransport {
  subscribeChatEvents(sessionId: string, after: number, handlers: ChatEventHandlers): () => void;
}

/** Opens an event stream; injectable so the parser can be tested without a server. */
export type StreamOpener = (url: string, init: StreamInit) => EventStream;

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

/**
 * The web default: a credentialed server-sent event feed for one session.
 * Frames arrive under the named `chat.event` type; other frame types are
 * heartbeats the browser transport surfaces and are ignored here.
 */
export function createCloudChatTransport(
  getClient: () => ApiClient = getAppClient,
  openStream: StreamOpener = (url, init) => platform.stream(url, init)
): ChatTransport {
  return {
    subscribeChatEvents(sessionId, after, { onEvent, onError }) {
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
              onError?.(frame);
              continue;
            }
            onEvent(frame);
          }
        } catch (error) {
          if (!closed) onError?.(error);
        }
      };
      void drain();
      return () => {
        if (closed) return;
        closed = true;
        feed.close();
      };
    },
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
