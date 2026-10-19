// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Live turn events (parity/specs/agents.md §10/§17, AGT-042). The backend
// serves one SSE channel per thread under the `chat.event` type; every
// frame is the contracted event union (assistant-events in
// @pidash/api-client). The channel opens through `core/platform`, so the
// web build reads it from fetch and the desktop build relays the same
// frames over its native stream — the view never branches on the host.
// Frames that do not parse are reported, never dropped as if they were a
// heartbeat.

import { AssistantEvent, assistantEventsUrl, parseAssistantEventFrames, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { platform, type EventStream, type StreamInit } from "../../../core/platform/index.js";

/** The SSE event name every assistant frame uses. */
export const ASSISTANT_EVENT_NAME = "chat.event";

export interface AssistantEventHandlers {
  /** One contracted event, in arrival order. */
  onEvent: (event: AssistantEvent) => void;
  /** An unreadable frame or a closed channel; the poll keeps the turn moving. */
  onError?: (error: unknown) => void;
}

/** Opens a channel; injectable so frame handling is tested without a server. */
export type AssistantStreamOpener = (url: string, init: StreamInit) => EventStream;

/**
 * One channel payload to its parsed events. The platform reader hands over
 * the `data:` payload (one JSON frame); a raw SSE block with its `data:`
 * lines is still accepted so the parser can be exercised on wire text.
 */
export function readAssistantEvents(data: string): AssistantEvent[] {
  const frames = parseJson(data);
  const events: AssistantEvent[] = [];
  for (const frame of frames) {
    const parsed = AssistantEvent.safeParse(frame);
    if (!parsed.success) throw new Error("assistant event frame did not match its contract");
    events.push(parsed.data);
  }
  return events;
}

function parseJson(data: string): unknown[] {
  try {
    return [JSON.parse(data) as unknown];
  } catch {
    // A browser reader may still hand over a raw block with `data:` lines.
    if (!/^\s*(?::|data:|event:|id:|retry:)/m.test(data)) {
      throw new Error(`assistant event frame is not JSON: ${data.slice(0, 120)}`);
    }
    return parseAssistantEventFrames(data);
  }
}

export interface AssistantStreamSubscription {
  /** Stop reading and close the channel; safe to call twice. */
  close(): void;
}

/**
 * Delay after a channel ends or breaks. The server replays from `after`, and
 * applied delta sequences are deduped by the transcript, so reopening never
 * doubles text. Backing off a second keeps a dead backend cheap.
 */
export const ASSISTANT_RECONNECT_MS = 1000;

/** Subscribe to one thread's live events and return the unsubscribe handle. */
export function subscribeAssistantEvents(
  workspaceSlug: string,
  threadId: string,
  after: number,
  handlers: AssistantEventHandlers,
  getClient: () => ApiClient = getAppClient,
  openStream: AssistantStreamOpener = (url, init) => platform.stream(url, init)
): AssistantStreamSubscription {
  let closed = false;
  let current: EventStream | null = null;
  let reconnect: ReturnType<typeof setTimeout> | null = null;

  // Only ever called from this subscription's own timer while it is open:
  // `close` cancels that timer, so a closed channel cannot be reopened.
  const open = (): void => {
    const feed = openStream(assistantEventsUrl(getClient(), workspaceSlug, threadId, after), {
      method: "GET",
      headers: { Accept: "text/event-stream" },
    });
    current = feed;
    void (async () => {
      try {
        for await (const frame of feed.messages()) {
          if (closed) return;
          if (frame.event !== ASSISTANT_EVENT_NAME) continue;
          try {
            for (const event of readAssistantEvents(frame.data)) handlers.onEvent(event);
          } catch (error) {
            handlers.onError?.(error);
          }
        }
      } catch (error) {
        if (!closed) handlers.onError?.(error);
      }
      if (closed) return;
      // The channel ended on its own (proxy timeout, server restart, a
      // finished response). Reopen it; the transcript dedupes the replay.
      current = null;
      reconnect = setTimeout(open, ASSISTANT_RECONNECT_MS);
    })();
  };

  open();
  return {
    close: () => {
      if (closed) return;
      closed = true;
      if (reconnect !== null) clearTimeout(reconnect);
      current?.close();
      current = null;
    },
  };
}
