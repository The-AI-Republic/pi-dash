// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation writes (parity/specs/agents.md §10, AGT-042/AGT-043): send
// stores the user row and queues a turn, stop asks the server to end it.
// Both leave the transcript stream to show the answer; the send also
// revalidates the thread list, because the first message titles an untitled
// thread and moves it to the top (AGT-054).

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  assistantErrorFrom,
  cancelAssistantTurn,
  sendAssistantMessage,
  type AssistantSendResponse,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { assistantKeys } from "./keys.js";
import { threadKey } from "./threadKeys.js";

/** Shown when a send fails without an assistant-shaped refusal. */
export const SEND_FAILED_FALLBACK = "Unable to send message";

/**
 * The inline line for a failed send: the assistant detail when the server
 * sent one, then its machine code, then a generic sentence. A busy-turn
 * conflict has no detail, so its code is what the composer shows.
 */
export function sendFailureLine(error: unknown): string {
  const failure = assistantErrorFrom(error);
  if (failure !== null) {
    if (failure.detail.trim() !== "") return failure.detail;
    if (failure.code.trim() !== "") return failure.code;
  }
  if (error instanceof Error && error.message.trim() !== "") return error.message;
  return SEND_FAILED_FALLBACK;
}

export interface SendMessageVars {
  workspaceSlug: string;
  threadId: string;
  content: string;
}

/** Post one message and refresh the thread list; the caller streams the answer. */
export function useSendMessage() {
  const queryClient = useQueryClient();
  return useMutation<AssistantSendResponse, Error, SendMessageVars>({
    mutationFn: ({ workspaceSlug, threadId, content }) =>
      sendAssistantMessage(getAppClient(), workspaceSlug, threadId, content),
    onSuccess: (_data, { workspaceSlug }) => {
      // A list read started before this send could still land and report the
      // old `has_active_turn`; drop it so the view only trusts a read of the
      // state after the turn was queued, then ask for that read.
      const threads = assistantKeys.threads(workspaceSlug);
      void queryClient
        .cancelQueries({ queryKey: threads })
        .then(() => queryClient.invalidateQueries({ queryKey: threads }));
    },
    onError: (_error, { workspaceSlug }) => {
      // A refused send changed nothing, but the first-message title may have
      // moved the thread; the stale read is harmless and simply refreshed.
      void queryClient.invalidateQueries({ queryKey: assistantKeys.threads(workspaceSlug) });
    },
  });
}

export interface StopTurnVars {
  workspaceSlug: string;
  threadId: string;
}

/**
 * Ask the server to cancel the running turn. Success is not the end of the
 * turn: the caller keeps the composer locked until the terminal row or
 * event arrives, so a resend cannot race the worker.
 */
export function useStopTurn() {
  const queryClient = useQueryClient();
  return useMutation<void, Error, StopTurnVars>({
    mutationFn: ({ workspaceSlug, threadId }) => cancelAssistantTurn(getAppClient(), workspaceSlug, threadId),
    onSuccess: (_data, { workspaceSlug, threadId }) => {
      void queryClient.invalidateQueries({ queryKey: threadKey(workspaceSlug, threadId) });
      // The cancel is acknowledged; re-read the list so its `has_active_turn`
      // can confirm the end even when the turn leaves no new row behind.
      void queryClient.invalidateQueries({ queryKey: assistantKeys.threads(workspaceSlug) });
    },
  });
}
