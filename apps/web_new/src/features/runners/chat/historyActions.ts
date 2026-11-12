// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Rules for the on-machine history actions (NEWFRONT-589, DESK-009). The
// dialogs and the page share these so a blank title, a delete of the
// conversation on screen and a "nothing left" answer all resolve the same
// way, in one place, with no DOM involved.

import type { ChatSession } from "@pidash/api-client";

import { sortSessionsByActivity } from "./chatState.js";

/** A blank title would make the entry unreadable, so it is refused first. */
export const HISTORY_TITLE_REQUIRED = "Enter a name for this conversation.";

/** Longest title the history column can show without eliding it away. */
export const HISTORY_TITLE_MAX = 120;

export function historyTitleError(raw: string): string | null {
  const trimmed = raw.trim();
  if (trimmed === "") return HISTORY_TITLE_REQUIRED;
  if (trimmed.length > HISTORY_TITLE_MAX) return `Keep the name under ${HISTORY_TITLE_MAX} characters.`;
  return null;
}

/**
 * The list a deletion resolves against. Before the first history read
 * answers there is nothing to resolve, and an empty list is the same answer
 * as no list at all.
 */
export function remainingSessions(sessions: readonly ChatSession[] | undefined): readonly ChatSession[] {
  return sessions ?? [];
}

/**
 * Which conversation stays selected after one is deleted: the same one when
 * it was not the row removed, otherwise the most recently active survivor,
 * or `null` when the history is now empty.
 */
export function selectionAfterDelete(
  sessions: readonly ChatSession[],
  deletedId: string,
  activeId: string | null
): string | null {
  if (activeId !== null && activeId !== deletedId) return activeId;
  const remaining = sortSessionsByActivity(sessions.filter((session) => session.id !== deletedId));
  return remaining[0]?.id ?? null;
}

/**
 * What the confirmation dialog says it will remove: the bare action for one
 * conversation, and the counted sentence for the whole account. Kept here
 * so the copy can be asserted without rendering the dialog.
 */
export const DELETE_CHAT_TITLE = "Delete this chat?";
export const DELETE_CHAT_BODY =
  "The messages in this conversation are removed from this computer. Other conversations and other signed-in accounts keep theirs.";
export const CLEAR_HISTORY_TITLE = "Clear the on-machine history?";

export function clearHistoryBody(count: number): string {
  if (count === 0) return "There is nothing stored for this account yet.";
  const noun = count === 1 ? "conversation" : "conversations";
  return `All ${count} ${noun} for this account are removed from this computer, messages included. Other accounts keep theirs.`;
}

/** Button wording, shared by the dialogs and the row menu. */
export const RENAME_ACTION = "Rename";
export const DELETE_ACTION = "Delete";
export const CLEAR_ACTION = "Clear history";
