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
  terminalRow,
  transcriptRows,
  type TranscriptState,
} from "../lib/turn.js";

/** Transcript page size: the server bounds a thread below this. */
export const TRANSCRIPT_PAGE_SIZE = 200;

/** Poll cadence for a turn that is visibly in flight. */
export const TURN_POLL_MS = 1000;

/** Fallback line when a failure event carries neither detail nor code. */
export const TURN_FAILED_FALLBACK = "The assistant could not finish this reply.";

/**
 * Whether the stored transcript still ends mid-turn. A turn closes with an
 * assistant or error row; a trailing user row, a streaming row and a tool
 * row all mean work may still be running (AGT-042).
 */
function turnPending(rows: readonly AssistantMessage[] | undefined): boolean {
  return rows !== undefined && rows.length > 0 && !replySettled(rows);
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
  const reduce = React.useCallback((current: TranscriptState, action: Action): TranscriptState => {
    switch (action.type) {
      case "reset":
        return EMPTY_TRANSCRIPT;
      case "page":
        return mergeTranscript(current, action.rows);
      case "message":
        return mergeMessage(current, action.message);
      case "event":
        return applyAssistantEvent(current, action.event);
    }
  }, []);
  const [state, dispatch] = React.useReducer(reduce, EMPTY_TRANSCRIPT);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  // A terminal lifecycle event is authoritative even before the poll catches
  // up: the stored page can still end on the user row for a moment, and
  // reading that as "in flight" would keep the composer locked after the
  // server is done. State, not a ref: the event often arrives while `busy` is
  // already false (a cold load), and unlocking has to re-render on its own.
  const [terminalSeen, setTerminalSeen] = React.useState(false);
  // The turn this view is following. The channel always replays from zero, so
  // an older turn's `turn_completed` can arrive after a new send; matching the
  // id keeps that replay from unlocking a turn that has not started yet.
  const turnRef = React.useRef<string | null>(null);
  // The newest row's turn, and whether the transcript still looks mid-turn.
  // Before any start has been seen these are the only way to tell a terminal
  // for the turn on screen from the replay of an older one.
  const tailTurnRef = React.useRef<string | null>(null);
  const pendingRef = React.useRef(false);
  // Highest `turn_started` sequence applied. The replay is ordered, so a
  // start at or below it describes a turn this view already saw.
  const startSeqRef = React.useRef(0);
  // The turn a send queued and this view is still waiting to see start. Until
  // its own start arrives, any other start in the replay belongs to an
  // earlier turn and must not claim the view; the closure's `busy` cannot
  // police that because the subscription is not re-created on every render.
  const awaitingStartRef = React.useRef<string | null>(null);

  // When the latest turn started, on this clock. A thread-list read taken at
  // or after this point is the server's answer about that turn; an older one
  // predates it and cannot settle anything.
  const [startedAt, setStartedAt] = React.useState(0);

  const messages = React.useMemo(() => transcriptRows(state), [state]);

  // The thread list carries `has_active_turn`, the only server read that
  // says a turn is over even when stopping added no transcript row (a cancel
  // before the assistant row exists, or after a tool row). The rail shares
  // this cache and its screens read the same list.
  const pending = turnPending(messages);
  // Poll the list while a turn looks in flight: a local start (busy) and a
  // trailing user or tool row from a cold load both need the server's answer.
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
  if (terminalSeen) {
    // A terminal that a live turn or the turn on screen accepted is final:
    // the server said so, and a replayed terminal never reaches this point.
    running = false;
  } else if (busy) {
    // A fresh read that says no turn runs settles a stop whose cancel
    // finalized the turn without adding a row.
    running = !serverSettled;
  } else {
    // No start seen yet (a cold load) or the tail is the only evidence: the
    // server's `has_active_turn` wins, and a tail that lacks a closing reply
    // (user, streaming or tool row) stays optimistic until the first read.
    running = serverActive === true || (!serverFresh && pending);
  }

  const page = useQuery({
    ...assistantTranscriptQueryOptions(getAppClient(), workspaceSlug, threadId, TRANSCRIPT_PAGE_SIZE),
    refetchInterval: running ? TURN_POLL_MS : false,
  });

  // The list poll continues while a turn is shown as running or an optimistic
  // cold load needs the server's answer; once it says "no turn", both the
  // transcript and the list can go quiet.
  const awaitingServer = running && !serverSettled;
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
        if (row?.has_active_turn !== false) return;
        setSettledFor(turnStarted);
        // The turn ended; read the finalize that closed its open row.
        void queryClient.invalidateQueries({ queryKey: transcriptKey(workspaceSlug, threadId) });
      });
    };
    read();
    const timer = setInterval(read, TURN_POLL_MS);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [awaitingServer, queryClient, refetchThreads, threadId, workspaceSlug]);

  React.useEffect(() => {
    dispatch({ type: "reset" });
    setTerminalSeen(false);
    turnRef.current = null;
    awaitingStartRef.current = null;
    startSeqRef.current = 0;
    setBusy(false);
    setError(null);
    setStartedAt(0);
  }, [workspaceSlug, threadId]);

  React.useEffect(() => {
    if (page.data !== undefined) dispatch({ type: "page", rows: page.data });
  }, [page.data]);

  // The rows as the event handler sees them. A replayed `turn_started` is
  // recognisable by its data, not its sequence: it names a turn whose answer
  // the page already holds. Kept in a ref because the channel subscription
  // is not re-created on every render.
  const rowsRef = React.useRef(messages);
  rowsRef.current = messages;
  const tail = messages.at(-1);
  tailTurnRef.current = typeof tail?.turn_id === "string" ? tail.turn_id : null;
  pendingRef.current = pending;

  const markStarted = React.useCallback((turnId: string) => {
    setTerminalSeen(false);
    turnRef.current = turnId;
    setStartedAt(Date.now());
    setBusy(true);
  }, []);

  const markTerminal = React.useCallback((turnId: string) => {
    setTerminalSeen(true);
    if (turnId !== "") turnRef.current = turnId;
    awaitingStartRef.current = null;
    setBusy(false);
  }, []);

  // Lifecycle events carry the turn they describe. The channel replays from
  // zero on every (re)connect, so one from a previous turn must neither end
  // nor start the turn this view is queued for; the send response and the
  // newest `turn_started` are what name the current turn (AGT-042).
  const followsTurn = React.useCallback((event: AssistantEvent): string | null => {
    const turnId = (event.payload as { turn_id?: unknown }).turn_id;
    if (typeof turnId !== "string") return "";
    if (turnRef.current !== null) return turnId === turnRef.current ? turnId : null;
    // No start seen yet (a cold load): a terminal only describes the turn on
    // screen when the transcript still looks mid-turn, the tail either has no
    // turn of its own or belongs to that turn, and the page holds no reply
    // for it yet. A settled transcript means the server's active read, not a
    // replayed terminal, decides; so does a tail already answered by that
    // same turn.
    if (!pendingRef.current) return null;
    if (tailTurnRef.current !== null && tailTurnRef.current !== turnId) return null;
    return rowsRef.current.some((message) => terminalRow(message, turnId)) ? null : turnId;
  }, []);

  const refresh = React.useCallback(() => {
    void queryClient.invalidateQueries({ queryKey: transcriptKey(workspaceSlug, threadId) });
  }, [queryClient, threadId, workspaceSlug]);

  React.useEffect(() => {
    const subscription = subscribeAssistantEvents(workspaceSlug, threadId, 0, {
      onEvent: (event: AssistantEvent) => {
        switch (event.kind) {
          case "turn_started": {
            const turnId = (event.payload as { turn_id: string }).turn_id;
            // A replay from zero repeats earlier starts first; only a start
            // past the last one applied describes a newer turn. While a send
            // waits for its own start, the replayed starts all name earlier
            // turns and must not take the view from it.
            if (event.seq <= startSeqRef.current) break;
            const awaiting = awaitingStartRef.current;
            if (awaiting !== null && turnId !== awaiting) break;
            // A cold-load replay serves the starts of turns whose answers the
            // page already holds; those are history, and replaying them would
            // let their terminals unlock a turn the server says is running.
            const finished = rowsRef.current.some((message) => terminalRow(message, turnId));
            if (finished && awaiting !== turnId) break;
            startSeqRef.current = event.seq;
            if (awaiting === turnId) awaitingStartRef.current = null;
            markStarted(turnId);
            break;
          }
          case "assistant_delta":
            dispatch({ type: "event", event });
            break;
          case "message_created":
          case "message_completed":
          case "tool_call":
          case "tool_result":
            dispatch({ type: "event", event });
            break;
          case "turn_failed": {
            const turnId = followsTurn(event);
            if (turnId === null) break;
            setError(failureDetail(event));
            markTerminal(turnId);
            refresh();
            void refetchThreads();
            break;
          }
          case "turn_completed":
          case "turn_cancelled": {
            const turnId = followsTurn(event);
            if (turnId === null) break;
            setError(null);
            markTerminal(turnId);
            refresh();
            void refetchThreads();
            break;
          }
          default:
            break;
        }
      },
    });
    return () => subscription.close();
  }, [followsTurn, markStarted, markTerminal, refetchThreads, refresh, threadId, workspaceSlug]);

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
            awaitingStartRef.current = answer.turn.id;
            markStarted(answer.turn.id);
            refresh();
          },
          onError: (failure) => {
            setError(sendFailureLine(failure));
            onError?.(failure);
          },
        }
      );
    },
    [markStarted, refresh, sendMutation, threadId, workspaceSlug]
  );

  // The typed send has nowhere to report a failure; the inline error line
  // already carries it, so the callback is simply absent.
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
