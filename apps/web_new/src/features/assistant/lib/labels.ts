// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation labels (parity/specs/agents.md §11/§13). The rail and the
// dashboard card both name a conversation the same way, and an untitled
// thread gets a stable fallback instead of a blank row. Kept free of
// component imports so the home card does not pull the assistant views
// into the first paint.

import type { AssistantThread } from "@pidash/api-client";

import { UNTITLED_CONVERSATION } from "./copy.js";

/** Conversations a list shows: archived rows never render. */
export function visibleConversations(threads: readonly AssistantThread[] | undefined): AssistantThread[] {
  return (threads ?? []).filter((thread) => !thread.is_archived);
}

/** The row text for one conversation, falling back to the untitled label. */
export function conversationLabel(thread: Pick<AssistantThread, "title">): string {
  return thread.title.trim() === "" ? UNTITLED_CONVERSATION : thread.title;
}
