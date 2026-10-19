// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner-chat reads (NEWFRONT-549, RUN-026/032). The header's runner comes
// from the polled detail read so a state change shows without a reload,
// and the history panel from the runner's session list. Messages are read
// for the selected session only, because auto-select prefers a session
// that already has a transcript.

import { queryOptions, useQuery } from "@tanstack/react-query";
import { getChatSession, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { chatKeys } from "./keys.js";
import { fetchChatMessages, fetchChatRunner, fetchChatSessions } from "./transport.js";

/** Status changes appear live; the detail read re-polls on the list cadence. */
export const CHAT_RUNNER_POLL_INTERVAL_MS = 5_000;
/** History moves after turns and human actions, so the same short cadence. */
export const CHAT_SESSIONS_POLL_INTERVAL_MS = 5_000;

export function chatRunnerQueryOptions(client: ApiClient, workspaceSlug: string, runnerId: string) {
  return queryOptions({
    queryKey: chatKeys.runner(workspaceSlug, runnerId),
    queryFn: () => fetchChatRunner(client, runnerId),
    staleTime: 0,
    refetchInterval: CHAT_RUNNER_POLL_INTERVAL_MS,
  });
}

export function chatSessionsQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  workspaceId: string,
  runnerId: string
) {
  return queryOptions({
    queryKey: chatKeys.sessions(workspaceSlug, runnerId),
    queryFn: () => fetchChatSessions(client, workspaceId, runnerId),
    staleTime: 0,
    refetchInterval: CHAT_SESSIONS_POLL_INTERVAL_MS,
  });
}

export function chatSessionQueryOptions(client: ApiClient, workspaceSlug: string, sessionId: string) {
  return queryOptions({
    queryKey: [...chatKeys.all(workspaceSlug), "session", sessionId] as const,
    queryFn: () => getChatSession(client, sessionId),
    staleTime: 0,
  });
}

export function chatMessagesQueryOptions(client: ApiClient, workspaceSlug: string, sessionId: string) {
  return queryOptions({
    queryKey: chatKeys.messages(workspaceSlug, sessionId),
    queryFn: () => fetchChatMessages(client, sessionId),
    staleTime: 0,
  });
}

/** Header runner, polled. */
export function useChatRunner(workspaceSlug: string, runnerId: string, enabled = true) {
  return useQuery({ ...chatRunnerQueryOptions(getAppClient(), workspaceSlug, runnerId), enabled });
}

/** History sessions for the runner. */
export function useChatSessions(workspaceSlug: string, workspaceId: string, runnerId: string, enabled = true) {
  return useQuery({
    ...chatSessionsQueryOptions(getAppClient(), workspaceSlug, workspaceId, runnerId),
    enabled,
  });
}

/** Transcript of one session, read only once one is selected. */
export function useChatMessages(workspaceSlug: string, sessionId: string | null, enabled = true) {
  return useQuery({
    ...chatMessagesQueryOptions(getAppClient(), workspaceSlug, sessionId ?? "none"),
    enabled: enabled && sessionId !== null,
  });
}
