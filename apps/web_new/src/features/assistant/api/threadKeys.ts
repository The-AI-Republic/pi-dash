// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cache keys for one conversation (AGT-042/AGT-054). They sit apart from
// the workspace-level keys: the thread-list consumers (the rail and the
// dashboard card) never need the per-thread subtree, so keeping it here
// leaves their chunks untouched.

import { assistantKeys } from "./keys.js";

/** One conversation subtree: its stored page and live state. */
export const threadKey = (workspaceSlug: string, threadId: string) =>
  [...assistantKeys.all(workspaceSlug), "thread", threadId] as const;

/** One conversation's stored transcript page. */
export const transcriptKey = (workspaceSlug: string, threadId: string) =>
  [...threadKey(workspaceSlug, threadId), "messages"] as const;
