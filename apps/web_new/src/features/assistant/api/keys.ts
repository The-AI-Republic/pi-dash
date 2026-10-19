// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant query keys (Data layer). Conversation reads are shared by the
// landing, the assistant rail and the dashboard card, so they live under
// one workspace-scoped subtree.

import { workspaceScope } from "../../../core/query/client.js";

export const assistantKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "assistant"] as const,
  threads: (workspaceSlug: string) => [...assistantKeys.all(workspaceSlug), "threads"] as const,
  config: (workspaceSlug: string) => [...assistantKeys.all(workspaceSlug), "config"] as const,
  /** One conversation subtree (transcript, live state). */
  thread: (workspaceSlug: string, threadId: string) =>
    [...assistantKeys.all(workspaceSlug), "thread", threadId] as const,
};

/** One conversation's stored transcript page. */
export const transcriptKey = (workspaceSlug: string, threadId: string) =>
  [...assistantKeys.thread(workspaceSlug, threadId), "messages"] as const;
