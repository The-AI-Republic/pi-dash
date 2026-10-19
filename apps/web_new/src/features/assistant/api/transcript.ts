// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stored transcript read (parity/specs/agents.md §10, AGT-042). Kept apart
// from the shared thread-list and config reads: only the conversation body
// needs the message contract, and the thread-list consumers sit in other
// features' eager chunks.

import { queryOptions } from "@tanstack/react-query";
import { listAssistantMessages, type ApiClient } from "@pidash/api-client";

import { transcriptKey } from "./threadKeys.js";

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
