// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Comment @-mention picker (parity/specs/comments.md §6.1, CMT-019).
// The comment composer and the inline edit form share this popup: while
// an @-query is live it offers the matching workspace members under a
// Users section, at most a handful, each with an avatar. Picking a row
// inserts the stored mention at the caret; Enter picks the first row
// and Escape dismisses the popup until the query changes.

import { Avatar } from "@pidash/kit";
import * as React from "react";

import type { RichTextEditorHandle } from "../../../shared/editor/index.js";
import { COMMENT_MENTION_LIMIT, useCommentMentionSearch, type CommentMentionPerson } from "./api.js";
import { swallowCommentEnter } from "./model.js";

/** Accessible name of the suggestion popup. */
export const MENTION_SUGGESTIONS_LABEL = "Mention suggestions";
/** Section heading the people suggestions render under. */
export const MENTION_USERS_SECTION = "Users";

export interface CommentMentionPicker {
  workspaceSlug: string;
  projectId: string;
  /** Live editor handle, or null once the surface unmounts. */
  getHandle: () => RichTextEditorHandle | null;
  /** A post/save or an upload is in flight. */
  busy: boolean;
  /** The draft holds neither words nor a stored content node. */
  empty: boolean;
}

export interface CommentMentionPickerState {
  /** People the live query matched, capped for the popup. */
  people: CommentMentionPerson[];
  /** The popup shows and Enter would pick its first row. */
  mentionOpen: boolean;
  /** Reports the editor's live @-query (or its absence). */
  onMentionQuery: (query: string | null) => void;
  /** Insert one person at the caret and close the popup. */
  pick: (person: CommentMentionPerson) => void;
  /** Insert the first suggestion, when the popup shows one. */
  pickFirst: () => void;
  /** Close the popup until the query changes. */
  dismiss: () => void;
  /** Forget the query entirely, for a cleared draft. */
  reset: () => void;
  /** Editor-level Enter guard: true swallows the key (CMT-011). */
  handleEditorKeyDown: (event: KeyboardEvent) => boolean;
}

/** @-mention popup state both comment forms share (CMT-019). */
export function useCommentMentionPicker({
  workspaceSlug,
  projectId,
  getHandle,
  busy,
  empty,
}: CommentMentionPicker): CommentMentionPickerState {
  const [mentionQuery, setMentionQuery] = React.useState<string | null>(null);
  const [dismissed, setDismissed] = React.useState<string | null>(null);
  const search = useCommentMentionSearch(workspaceSlug, projectId, mentionQuery ?? "", mentionQuery !== null);
  const people = (search.data ?? []).slice(0, COMMENT_MENTION_LIMIT);
  const mentionOpen = mentionQuery !== null && dismissed !== mentionQuery && people.length > 0;

  const onMentionQuery = React.useCallback(
    (query: string | null) => {
      setMentionQuery(query);
      // A changed query re-arms the popup; the dismissed text staying
      // put keeps an Escape dismissal in force.
      if (query !== dismissed) setDismissed(null);
    },
    [dismissed]
  );

  const pick = React.useCallback(
    (person: CommentMentionPerson) => {
      getHandle()?.insertMention({ id: person.id, name: person.name });
      setMentionQuery(null);
      setDismissed(null);
    },
    [getHandle]
  );

  const pickFirst = React.useCallback(() => {
    const first = people[0];
    if (first !== undefined) pick(first);
  }, [people, pick]);

  const dismiss = React.useCallback(() => {
    setDismissed(mentionQuery);
  }, [mentionQuery]);

  const reset = React.useCallback(() => {
    setMentionQuery(null);
    setDismissed(null);
  }, []);

  const handleEditorKeyDown = React.useCallback(
    (event: KeyboardEvent) => {
      if (event.key !== "Enter" || event.shiftKey || event.ctrlKey || event.metaKey || event.isComposing) return false;
      return swallowCommentEnter({ mentionOpen, busy, empty });
    },
    [mentionOpen, busy, empty]
  );

  return { people, mentionOpen, onMentionQuery, pick, pickFirst, dismiss, reset, handleEditorKeyDown };
}

/** The @ popup: matching people under the Users section. */
export function CommentMentionSuggestions({
  people,
  onPick,
}: {
  people: CommentMentionPerson[];
  onPick: (person: CommentMentionPerson) => void;
}): React.ReactElement {
  return (
    <ul
      aria-label={MENTION_SUGGESTIONS_LABEL}
      className="shadow-popover flex flex-col gap-px rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-1)"
    >
      <li className="text-caption px-(--space-2) py-(--space-1) font-medium text-(--text-muted)">
        {MENTION_USERS_SECTION}
      </li>
      {people.map((person) => (
        <li key={person.id}>
          <button
            type="button"
            onMouseDown={(event) => {
              event.preventDefault();
              onPick(person);
            }}
            className="text-body flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left text-(--text) hover:bg-(--subtle)"
          >
            <Avatar name={person.name} {...(person.avatarUrl === null ? {} : { src: person.avatarUrl })} size="small" />
            <span className="min-w-0 flex-1 truncate">{person.name}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}
