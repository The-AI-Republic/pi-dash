// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stored transcript read (parity/specs/agents.md §10/§11, AGT-042/AGT-055).
// Kept apart from the shared thread-list and config reads: only the
// conversation body needs the message contract, and the thread-list
// consumers sit in other features' eager chunks. Long conversations page
// through the cursor read one page at a time, so every row renders exactly
// once no matter how far the transcript runs.

import { queryOptions } from "@tanstack/react-query";
import { listAssistantMessages, type ApiClient, type AssistantMessageList } from "@pidash/api-client";

import { transcriptKey } from "./threadKeys.js";

/**
 * Read the whole stored transcript through the cursor read: each page
 * starts after the previous page's last sequence number, and a short page
 * ends the walk. Threads are capped server-side, so the loop is bounded.
 */
async function readTranscript(
  client: ApiClient,
  workspaceSlug: string,
  threadId: string,
  limit: number
): Promise<AssistantMessageList> {
  const out: AssistantMessageList = [];
  let after = 0;
  for (;;) {
    const batch = await listAssistantMessages(client, workspaceSlug, threadId, { after, limit });
    for (const row of batch) {
      out.push(row);
      after = row.seq;
    }
    if (batch.length < limit) break;
  }
  return out;
}

/** The stored transcript, oldest first, across however many pages it takes. */
export function assistantTranscriptQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  threadId: string,
  limit: number
) {
  return queryOptions({
    queryKey: transcriptKey(workspaceSlug, threadId),
    queryFn: () => readTranscript(client, workspaceSlug, threadId, limit),
    staleTime: 0,
  });
}
