// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Person mentions in the work-item description (parity/specs/issues.md
// §11, ISS-127). The stored markup is the shared `mention-component`
// element carrying the member id, the entity kind and the display name,
// so descriptions written here render in every client. This module holds
// the pure pieces: the tag and entity names, the attribute builder, the
// @-query reader and the content test. The Tiptap node lives in
// mentionNode.ts behind import() because every Tiptap package is heavy.

export const MENTION_TAG = "mention-component";
export const MENTION_ENTITY = "user_mention";

/** One picked person, as the mention node stores it. */
export interface MentionTarget {
  id: string;
  name: string;
}

/** Attributes one mention node carries for a picked person. */
export function mentionAttributes(target: MentionTarget): Record<string, string> {
  return {
    id: target.id,
    entity_identifier: target.id,
    entity_name: MENTION_ENTITY,
    // The display name rides along so read surfaces can show the
    // person without a member lookup (ISS-127); older bodies carry no
    // name and fall back to the identifier.
    "data-name": target.name,
  };
}

/** The text a stored mention shows: its stored name, else its identifier. */
export function mentionLabel(name: string | null | undefined, identifier: string | null | undefined): string {
  const stored = name?.trim() ?? "";
  if (stored !== "") return `@${stored}`;
  return `@${identifier ?? ""}`;
}

/** The open @-query before the caret, or null when none is active. */
export interface MentionQuery {
  /** Text typed after the @, possibly empty. */
  text: string;
}

/**
 * Read the mention query the caret currently sits in: the nearest @ on
 * the same text run, with no whitespace between it and the caret. An
 * empty query right after a typed @ opens the list.
 */
export function mentionQueryAt(textBeforeCaret: string): MentionQuery | null {
  const at = textBeforeCaret.lastIndexOf("@");
  if (at === -1) return null;
  const tail = textBeforeCaret.slice(at + 1);
  if (/\s/.test(tail)) return null;
  if (at > 0 && !/\s/.test(textBeforeCaret.slice(at - 1, at))) return null;
  return { text: tail };
}

/** Stored HTML mentions count as content even without words (ISS-127). */
export const MENTION_CONTENT_TAGS = ["img", MENTION_TAG, "image-component"] as const;
