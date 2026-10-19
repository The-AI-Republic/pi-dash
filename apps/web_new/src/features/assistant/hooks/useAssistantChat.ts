// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Live conversation state for one thread (parity/specs/agents.md §10).
// Three feeds describe one turn: the stored transcript page is the durable
// record, the event channel adds token deltas and lifecycle signals as they
// arrive, and a one-second poll runs while a turn is visibly in flight so an
// answer still lands when the channel stays silent (AGT-042). Stopping only
// asks the server to cancel; the composer stays locked until a terminal row
// or event arrives, so a resend can never race the worker.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { AssistantEvent, AssistantMessage } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { subscribeAssistantEvents } from "../api/events.js";
import { assistantThreadsQueryOptions } from "../api/queries.js";
import { transcriptKey } from "../api/threadKeys.js";
import { assistantTranscriptQueryOptions } from "../api/transcript.js";
import { sendFailureLine, useSendMessage, useStopTurn } from "../api/threadMutations.js";
import {
  EMPTY_TRANSCRIPT,
  applyAssistantEvent,
  mergeMessage,
  mergeTranscript,
  replySettled,
  transcriptRows,
  type TranscriptState,
} from "../lib/turn.js";

/** Transcript page size: the server bounds a thread below this. */
export const TRANSCRIPT_PAGE_SIZE = 200;

/** Poll cadence for a turn that is visibly in flight. */
export const TURN_POLL_MS = 1000;

/** Fallback line when a failure event carries neither detail nor code. */
export const TURN_FAILED_FALLBACK = "The assistant could not finish this reply.";

function turnPending(rows: readonly AssistantMessage[] | undefined): boolean {
  const last = rows?.at(-1);
  if (last === undefined) return false;
  if (last.status === "streaming") return true;
  return last.role === "user";
}

function eventMessage(event: AssistantEvent): AssistantMessage | undefined {
  return (event.payload as { message?: AssistantMessage }).message;
}

function failureDetail(event: AssistantEvent): string {
  const payload = event.payload as { detail?: unknown; error_code?: unknown };
  if (typeof payload.detail === "string" && payload.detail.trim() !== "") return payload.detail;
  if (typeof payload.error_code === "string" && payload.error_code.trim() !== "") return payload.error_code;
  return TURN_FAILED_FALLBACK;
}

type Action =
  | { type: "reset" }
  | { type: "page"; rows: readonly AssistantMessage[] }
  | { type: "message"; message: AssistantMessage }
  | { type: "event"; event: AssistantEvent };

export interface AssistantChat {
  /** Rows oldest first, ready to render. */
  messages: AssistantMessage[];
  /** A turn is running: stop replaces send and the poll is on. */
  running: boolean;
  /** A send is posting: the box locks until it resolves. */
  sending: boolean;
  /** Inline send or turn failure; null when there is none. */
  error: string | null;
  /** Whether the stored page resolved (data or a readable failure). */
  loaded: boolean;
  send: (content: string) => void;
  /** Deliver the landing handoff; failures go to `onError` for a notice. */
  sendHandoff: (content: string, onError: (error: unknown) => void) => void;
  stop: () => void;
}

export function useAssistantChat(workspaceSlug: string, threadId: string): AssistantChat {
  const queryClient = useQueryClient();
  const seen = React.useRef<Set<number>>(new Set());
  const reduce = React.useCallback((current: TranscriptState, action: Action): TranscriptState => {
    switch (action.type) {
      case "reset":
        return EMPTY_TRANSCRIPT;
      case "page":
        return mergeTranscript(current, action.rows);
      case "message":
        return mergeMessage(current, action.message);
      case "event":
        return applyAssistantEvent(current, action.event, seen.current);
    }
  }, []);
  const [state, dispatch] = React.useReducer(reduce, EMPTY_TRANSCRIPT);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  // A terminal event is authoritative even before the poll catches up: the
  // stored page can still end on the user row for a moment, and reading that
  // as "in flight" would keep the composer locked after the server is done.
  // State, not a ref: the event often arrives while `busy` is already false
  // (a cold load), and unlocking has to re-render on its own.
  const [terminalSeen, setTerminalSeen] = React.useState(false);
  // Sequence the turn started at: a reply already in the transcript when the
  // turn began says nothing about this turn, so the settle check ignores it.
  const startedAfter = React.useRef(0);

  // When the latest turn started, on this clock. A thread-list read taken at
  // or after this point is the server's answer about that turn; an older one
  // predates it and cannot settle anything.
  const [startedAt, setStartedAt] = React.useState(0);

  const messages = React.useMemo(() => transcriptRows(state), [state]);
  const stateRef = React.useRef(state);
  stateRef.current = state;

  // The thread list carries `has_active_turn`, the only server read that
  // says a turn is over even when stopping added no transcript row (a cancel
  // before the assistant row exists, or after a tool row). The rail shares
  // this cache and its screens read the same list.
  const pending = turnPending(messages) && !terminalSeen;
  // Poll the list while a turn looks in flight: a local start (busy) and a
  // trailing user row from a cold load both need the server's answer.
  const threads = useQuery({
    ...assistantThreadsQueryOptions(getAppClient(), workspaceSlug),
    refetchInterval: false,
  });
  const threadRow = threads.data?.find((thread) => thread.id === threadId);
  const serverFresh = threadRow !== undefined && threads.dataUpdatedAt >= startedAt;
  const serverActive = threadRow?.has_active_turn;
  const [settledFor, setSettledFor] = React.useState<number | null>(null);
  // A local turn (busy) settles only on a list read requested after it
  // began and that says no turn runs: a cached read from before the send
  // still reports the thread as idle and must not clear it.
  const serverSettled = busy
    ? serverFresh && serverActive === false && settledFor === startedAt
    : serverFresh && serverActive === false;
  let running = false;
  if (!terminalSeen) {
    if (busy) {
      // A fresh read that says no turn runs settles a stop whose cancel
      // finalized the turn without adding a row.
      running = !serverSettled;
    } else if (pending) {
      // No local start (a cold load, another tab). A trailing user row is
      // ambiguous: the server decides, and stays optimistic until it answers.
      running = serverActive === true || !serverFresh;
    }
  }

  const page = useQuery({
    ...assistantTranscriptQueryOptions(getAppClient(), workspaceSlug, threadId, TRANSCRIPT_PAGE_SIZE),
    refetchInterval: running ? TURN_POLL_MS : false,
  });

  // The list poll continues only while a local start or an unconfirmed tail
  // user row still needs the server's answer; once it says "no turn", both
  // the transcript and the list can go quiet.
  const awaitingServer = busy || (pending && !serverSettled);
  const refetchThreads = threads.refetch;
  // Only a read requested after the turn began counts: a stale read still
  // reports the thread as idle and must not settle the turn it predates.
  // Started during render so the effect's first read is stamped correctly.
  const startedRef = React.useRef(startedAt);
  if (startedAt > startedRef.current) startedRef.current = startedAt;
  React.useEffect(() => {
    if (!awaitingServer) return;
    let active = true;
    const read = (): void => {
      const turnStarted = startedRef.current;
      void refetchThreads().then((result) => {
        if (!active) return;
        const row = result.data?.find((thread) => thread.id === threadId);
        if (row?.has_active_turn === false) setSettledFor(turnStarted);
      });
    };
    read();
    const timer = setInterval(read, TURN_POLL_MS);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [awaitingServer, refetchThreads, threadId]);

  React.useEffect(() => {
    dispatch({ type: "reset" });
    seen.current = new Set();
    setTerminalSeen(false);
    startedAfter.current = 0;
    setBusy(false);
    setError(null);
    setStartedAt(0);
  }, [workspaceSlug, threadId]);

  React.useEffect(() => {
    if (page.data !== undefined) dispatch({ type: "page", rows: page.data });
  }, [page.data]);

  // Poll-side settle: a terminal reply at the tail ends the turn even when
  // no lifecycle event ever arrived, as long as it belongs to this turn.
  React.useEffect(() => {
    if (!busy || !replySettled(messages)) return;
    // `replySettled` guarantees a last row.
    const lastRow = messages.at(-1)!;
    if (lastRow.seq > startedAfter.current) setBusy(false);
  }, [busy, messages]);

  const markTurn = React.useCallback((next: "started" | "terminal") => {
    setTerminalSeen(next === "terminal");
    if (next === "started") {
      const newest = transcriptRows(stateRef.current).at(-1);
      startedAfter.current = newest?.seq ?? 0;
      setStartedAt(Date.now());
    }
    setBusy(next === "started");
  }, []);

  const refresh = React.useCallback(() => {
    void queryClient.invalidateQueries({ queryKey: transcriptKey(workspaceSlug, threadId) });
  }, [queryClient, threadId, workspaceSlug]);

  React.useEffect(() => {
    const subscription = subscribeAssistantEvents(workspaceSlug, threadId, 0, {
      onEvent: (event) => {
        switch (event.kind) {
          case "turn_started":
            markTurn("started");
            break;
          case "assistant_delta":
            dispatch({ type: "event", event });
            break;
          case "message_created":
          case "message_completed":
          case "tool_call":
          case "tool_result": {
            dispatch({ type: "event", event });
            const row = eventMessage(event);
            if (row !== undefined && replySettled([row])) markTurn("terminal");
            break;
          }
          case "turn_failed":
            setError(failureDetail(event));
            markTurn("terminal");
            refresh();
            break;
          case "turn_completed":
          case "turn_cancelled":
            setError(null);
            markTurn("terminal");
            refresh();
            break;
          default:
            break;
        }
      },
    });
    return () => subscription.close();
  }, [markTurn, refresh, threadId, workspaceSlug]);

  const sendMutation = useSendMessage();
  const stopMutation = useStopTurn();

  /** One send path for typed messages and the landing handoff. */
  const deliver = React.useCallback(
    (content: string, onError?: (failure: unknown) => void) => {
      const trimmed = content.trim();
      if (trimmed === "") return;
      setError(null);
      sendMutation.mutate(
        { workspaceSlug, threadId, content: trimmed },
        {
          onSuccess: (answer) => {
            dispatch({ type: "message", message: answer.message });
            markTurn("started");
            refresh();
          },
          onError: (failure) => {
            setError(sendFailureLine(failure));
            onError?.(failure);
          },
        }
      );
    },
    [markTurn, refresh, sendMutation, threadId, workspaceSlug]
  );

  const send = React.useCallback((content: string) => deliver(content), [deliver]);
  const sendHandoff = React.useCallback(
    (content: string, onError: (failure: unknown) => void) => deliver(content, onError),
    [deliver]
  );

  return {
    messages,
    running,
    sending: sendMutation.isPending,
    error,
    loaded: page.data !== undefined || page.isError,
    send,
    sendHandoff,
    stop: () => {
      // Deliberately keeps `busy`: the server confirms the stop by ending
      // the turn, not by accepting the cancel request.
      stopMutation.mutate({ workspaceSlug, threadId });
    },
  };
}
