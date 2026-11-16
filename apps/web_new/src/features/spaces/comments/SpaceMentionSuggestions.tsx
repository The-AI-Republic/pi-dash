// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public comment @-mention picker (parity/specs/space.md §3.10,
// SPC-054). The comment composer and the inline edit form share this
// popup: while an @-query with at least one character is live it offers
// the matching board members under a Users section, at most a handful,
// each with an avatar. Picking a row inserts the stored mention at the
// caret; Enter picks the first row and Escape dismisses the popup until
// the query changes. With no members or no match the popup closes — it
// never offers people who are not project members.

import { Avatar } from "@pidash/kit";
import type { ApiClient } from "@pidash/api-client";
import * as React from "react";

import type { RichTextEditorHandle } from "../../../shared/editor/index.js";
import { spaceMessage } from "../lib/spaceMessages.js";
import { useSpaceMentionPeople } from "./api.js";
import { isSpaceCommentDraftEmpty, spaceCommentEnterAction, type SpaceMentionPerson } from "./model.js";

export interface SpaceMentionPicker {
  client: ApiClient;
  anchor: string;
  /** Live editor handle, or null once the surface unmounts. */
  getHandle: () => RichTextEditorHandle | null;
  /** A post/save or an upload is in flight. */
  busy: boolean;
  /** The draft holds neither words nor a stored content node. */
  empty: boolean;
  /**
   * Post/save the draft. The live document HTML comes along because the
   * React snapshot lags the keystroke that filled it (SPC-053, ISS-197).
   */
  submit: (liveHtml: string | null) => void;
}

export interface SpaceMentionPickerState {
  /** People the live query matched, capped for the popup. */
  people: SpaceMentionPerson[];
  /** The popup shows and Enter would pick its first row. */
  mentionOpen: boolean;
  /** Reports the editor's live @-query (or its absence). */
  onMentionQuery: (query: string | null) => void;
  /** Insert one person at the caret and close the popup. */
  pick: (person: SpaceMentionPerson) => void;
  /** Insert the first suggestion, when the popup shows one. */
  pickFirst: () => void;
  /** Close the popup until the query changes. */
  dismiss: () => void;
  /** Forget the query entirely, for a cleared draft. */
  reset: () => void;
  /** Editor-level Enter guard: true swallows the key (SPC-053). */
  handleEditorKeyDown: (event: KeyboardEvent) => boolean;
}

/** @-mention popup state both public comment forms share (SPC-054). */
export function useSpaceMentionPicker({
  client,
  anchor,
  getHandle,
  busy,
  empty,
  submit,
}: SpaceMentionPicker): SpaceMentionPickerState {
  const [mentionQuery, setMentionQuery] = React.useState<string | null>(null);
  const [dismissed, setDismissed] = React.useState<string | null>(null);
  const live = mentionQuery !== null && mentionQuery !== "";
  const people = useSpaceMentionPeople(client, anchor, mentionQuery ?? "", live);
  const mentionOpen = live && dismissed !== mentionQuery && people.length > 0;

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
    (person: SpaceMentionPerson) => {
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

  // The editing surface calls this first for every key and always wins
  // the decision on a plain Enter: ProseMirror consumes Enter itself
  // (splitting the paragraph and preventing the default), so nothing
  // reaches the form unless this guard lets it (ISS-197, SPC-053). A
  // submit therefore acts here, at key time, from the live document:
  // the `empty` snapshot can still predate the keystroke that filled it.
  const handleEditorKeyDown = React.useCallback(
    (event: KeyboardEvent) => {
      if (event.key !== "Enter" || event.shiftKey || event.ctrlKey || event.metaKey || event.isComposing) return false;
      const liveHtml = getHandle()?.getHtml() ?? null;
      const action = spaceCommentEnterAction({ mentionOpen, busy, empty: isSpaceCommentDraftEmpty(empty, liveHtml) });
      if (action === "pick-mention") {
        pickFirst();
        return true;
      }
      if (action === "none") return true;
      submit(liveHtml);
      return true;
    },
    [getHandle, mentionOpen, busy, empty, pickFirst, submit]
  );

  return { people, mentionOpen, onMentionQuery, pick, pickFirst, dismiss, reset, handleEditorKeyDown };
}

/** The @ popup: matching people under the Users section. */
export function SpaceMentionSuggestions({
  people,
  onPick,
}: {
  people: SpaceMentionPerson[];
  onPick: (person: SpaceMentionPerson) => void;
}): React.ReactElement {
  return (
    <ul
      aria-label={spaceMessage("comments.mentionSuggestions")}
      data-testid="space-comment-mentions"
      className="shadow-popover flex flex-col gap-px rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-1)"
    >
      <li className="text-caption px-(--space-2) py-(--space-1) font-medium text-(--text-muted)">
        {spaceMessage("comments.mentionUsers")}
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
