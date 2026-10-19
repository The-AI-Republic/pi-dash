// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Live event feed for one chat session (NEWFRONT-551, RUN-029). Subscribes
// through the active transport, keeps the page's view of the turn — the
// streamed transcript, the activity strip, a transient error and the
// parked approval slot — and reopens the feed past the last frame it saw
// whenever the channel ends, because the web feed is a plain HTTP
// response rather than a self-reconnecting event source. Callbacks are
// held in a ref so a re-render never tears the feed down.

import * as React from "react";

import type { ChatEvent, ChatMessage } from "@pidash/api-client";

import {
  activityRows,
  appendEvent,
  applyDeltaOnce,
  isTerminalEvent,
  lastSequence,
  refreshesHistory,
  type ActivityRow,
} from "./events.js";
import { getChatTransport, type ChatSubscriptionHandlers } from "./transport.js";

/** First wait before reopening a dropped feed; doubles per failure. */
export const STREAM_RETRY_BASE_MS = 1_000;
/** Longest wait between reopen attempts. */
export const STREAM_RETRY_MAX_MS = 15_000;

/** Read an error worth showing to the person; a shrug reads as null. */
export function streamErrorMessage(error: unknown): string | null {
  if (error instanceof Error && error.message !== "") return error.message;
  if (typeof error === "string" && error !== "") return error;
  return null;
}

export interface ChatStreamCallbacks {
  /** Terminal frames ask the page to re-read messages and sessions. */
  onHistoryRefresh: () => void;
  /** The page's parked inline approval no longer applies. */
  onApprovalCleared: () => void;
}

export interface ChatStreamView {
  /** Transcript rows with the streamed deltas folded in. */
  messages: ChatMessage[];
  /** Activity strip rows, newest last. */
  activity: ActivityRow[];
  /** The last stream failure worth naming, until a frame or a dismissal clears it. */
  error: string | null;
  dismissError: () => void;
}

/**
 * Subscribe to a session's events. `baseMessages` is the saved transcript;
 * streamed deltas apply on top of it. The subscription restarts whenever
 * the session changes, and reopens with a sequence cursor after the
 * channel ends.
 */
export function useChatStream(
  sessionId: string | null,
  baseMessages: readonly ChatMessage[],
  callbacks: ChatStreamCallbacks
): ChatStreamView {
  const [liveMessages, setLiveMessages] = React.useState<ChatMessage[]>([]);
  const [events, setEvents] = React.useState<ChatEvent[]>([]);
  const [error, setError] = React.useState<string | null>(null);

  const appliedRef = React.useRef<Set<number>>(new Set());
  const eventsRef = React.useRef<ChatEvent[]>([]);
  const messagesRef = React.useRef<ChatMessage[]>([]);
  const baseRef = React.useRef<readonly ChatMessage[]>(baseMessages);
  baseRef.current = baseMessages;
  // A fresh array of equal rows must not reset the transcript (a re-render
  // with an inline default would otherwise loop); the signature changes
  // only when the saved rows themselves moved.
  const baseSignature = baseMessages.map((row) => `${row.id}:${row.content.length}:${row.status}:${row.seq}`).join("|");
  const subscribedAtRef = React.useRef(0);
  const callbacksRef = React.useRef(callbacks);
  callbacksRef.current = callbacks;

  const liveSessionRef = React.useRef<string | null>(sessionId);

  // The saved transcript is authoritative; the streamed rows layer on top.
  React.useEffect(() => {
    messagesRef.current = [...baseRef.current];
    setLiveMessages([...baseRef.current]);
  }, [baseSignature]);

  // The saved transcript arrives again after every history read; it must
  // not reopen the feed mid-turn (the cursor would replay and drop the
  // live bubble), so the subscription reads it through a ref.

  React.useEffect(() => {
    // A different conversation starts from scratch: no cursor, no strip,
    // no carried error, and a fresh "live since" point.
    liveSessionRef.current = sessionId;
    appliedRef.current = new Set();
    eventsRef.current = [];
    messagesRef.current = [...baseRef.current];
    subscribedAtRef.current = Date.now();
    setEvents([]);
    setError(null);
  }, [sessionId]);

  React.useEffect(() => {
    if (sessionId === null) return;
    const transport = getChatTransport();
    let unsubscribe: (() => void) | null = null;
    let closed = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let attempts = 0;

    const open = (): void => {
      const after = lastSequence(eventsRef.current, appliedRef.current);
      const handlers: ChatSubscriptionHandlers = {
        onEvent: (event) => {
          attempts = 0;
          setError(null);
          eventsRef.current = appendEvent(eventsRef.current, event);
          setEvents(eventsRef.current);
          if (event.kind === "assistant_delta") {
            const result = applyDeltaOnce({
              applied: appliedRef.current,
              event,
              subscribedAtMs: subscribedAtRef.current,
              messages: messagesRef.current,
              sessionId,
            });
            appliedRef.current = result.applied;
            messagesRef.current = result.messages;
            setLiveMessages(result.messages);
            return;
          }
          if (event.kind === "chat_approval_request") return;
          if (isTerminalEvent(event.kind)) callbacksRef.current.onApprovalCleared();
          if (refreshesHistory(event.kind)) callbacksRef.current.onHistoryRefresh();
        },
        onError: (cause) => {
          if (closed) return;
          const message = streamErrorMessage(cause);
          // A frame that did not parse is named; a transport error that
          // carries nothing (a plain channel drop) stays quiet.
          if (message !== null) setError(message);
          callbacksRef.current.onHistoryRefresh();
        },
        onEnd: () => {
          if (closed) return;
          attempts += 1;
          const delay = Math.min(STREAM_RETRY_MAX_MS, STREAM_RETRY_BASE_MS * 2 ** (attempts - 1));
          timer = setTimeout(open, delay);
        },
      };
      unsubscribe = transport.subscribeChatEvents(sessionId, after, handlers);
    };

    open();
    return () => {
      closed = true;
      if (timer !== null) clearTimeout(timer);
      unsubscribe?.();
    };
  }, [sessionId]);

  const dismissError = React.useCallback(() => setError(null), []);

  return {
    messages: liveMessages,
    activity: React.useMemo(() => activityRows(events), [events]),
    error,
    dismissError,
  };
}
