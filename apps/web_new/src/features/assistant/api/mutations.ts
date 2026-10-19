// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Starting a conversation (parity/specs/agents.md §9, AGT-038). The
// landing and the dashboard card both create an untitled thread and then
// hand the first message to the conversation view, which owns the send.
// The thread list is patched so the rail shows the new row immediately;
// the send path revalidates it and picks up the server-assigned title.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  createAssistantThread,
  isApiError,
  sendAssistantMessage,
  type AssistantSendResponse,
  type AssistantThread,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { assistantKeys } from "./keys.js";

/** Last-resort notice detail when a start failure carries no message. */
export const START_FAILED_FALLBACK = "Something went wrong. Please try again.";

/** The failure detail shown in the start notice: server detail, then message. */
export function startFailureDetail(error: unknown): string {
  if (isApiError(error) && error.message.trim() !== "") return error.message;
  if (error instanceof Error && error.message.trim() !== "") return error.message;
  return START_FAILED_FALLBACK;
}

interface StartConversationVars {
  workspaceSlug: string;
}

/** Create one untitled conversation and put it first in the cached list. */
export function useStartConversation() {
  const queryClient = useQueryClient();
  return useMutation<AssistantThread, Error, StartConversationVars>({
    mutationFn: ({ workspaceSlug }) => createAssistantThread(getAppClient(), workspaceSlug),
    onSuccess: (thread, { workspaceSlug }) => {
      queryClient.setQueryData<AssistantThread[]>(assistantKeys.threads(workspaceSlug), (previous) => [
        thread,
        ...(previous ?? []).filter((row) => row.id !== thread.id),
      ]);
    },
  });
}

/** Deliver the landing/card handoff once per thread; the thread slice streams the answer. */
export function useDeliverFirstMessage() {
  const queryClient = useQueryClient();
  return useMutation<AssistantSendResponse, Error, { workspaceSlug: string; threadId: string; content: string }>({
    mutationFn: ({ workspaceSlug, threadId, content }) =>
      sendAssistantMessage(getAppClient(), workspaceSlug, threadId, content),
    onSettled: (_data, _error, { workspaceSlug }) => {
      // The send titles an untitled thread; refresh so the rail follows.
      void queryClient.invalidateQueries({ queryKey: assistantKeys.threads(workspaceSlug) });
    },
  });
}
