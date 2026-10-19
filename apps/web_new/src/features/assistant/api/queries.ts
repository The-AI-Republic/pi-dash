// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant reads (parity/specs/agents.md §9). Every entry surface — the
// landing, the conversation rail and the dashboard card — decides between
// chat and setup from the same two reads: the workspace thread list and
// the user's model configuration. Guests are not disabled here: the server
// refuses them, and the refusal has to be observable (an absent list) for
// the rail to stay empty without a placeholder (AGT-041).

import { queryOptions, useQuery } from "@tanstack/react-query";
import { getLlmConfig, listAssistantMessages, listAssistantThreads, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS, REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { assistantKeys, transcriptKey } from "./keys.js";

/** The workspace's conversations, newest first (archived rows excluded server-side). */
export function assistantThreadsQueryOptions(client: ApiClient, workspaceSlug: string, enabled = true) {
  return queryOptions({
    queryKey: assistantKeys.threads(workspaceSlug),
    queryFn: () => listAssistantThreads(client, workspaceSlug),
    staleTime: LIST_STALE_TIME_MS,
    enabled,
  });
}

export function useAssistantThreads(workspaceSlug: string, enabled = true) {
  return useQuery(assistantThreadsQueryOptions(getAppClient(), workspaceSlug, enabled));
}

/** The user's model configuration; absence reads back as the blank default. */
export function assistantConfigQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: assistantKeys.config(workspaceSlug),
    queryFn: () => getLlmConfig(client),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

export function useAssistantConfig(workspaceSlug: string) {
  return useQuery(assistantConfigQueryOptions(getAppClient(), workspaceSlug));
}

/** One page of the stored transcript, oldest first. */
export function assistantTranscriptQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  threadId: string,
  limit: number
) {
  return queryOptions({
    queryKey: transcriptKey(workspaceSlug, threadId),
    queryFn: () => listAssistantMessages(client, workspaceSlug, threadId, { limit }),
    staleTime: 0,
  });
}
