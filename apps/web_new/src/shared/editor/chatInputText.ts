// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The plain-text model behind the chat composer input (ChatInput). The
// draft is a string; the editor holds it as one paragraph whose line
// breaks are hard breaks, so string → document → string is the identity
// and a draft set from outside (a suggestion chip, a dictation
// transcript, a cleared box after send) lands exactly as written. Kept
// free of Tiptap so the eager chunk and node tests can use it.

/** The JSON shape Tiptap's setContent accepts for a plain-text draft. */
export interface ChatInputDoc {
  type: "doc";
  content: [
    {
      type: "paragraph";
      content: ({ type: "text"; text: string } | { type: "hardBreak" })[];
    },
  ];
}

/** The editor document for a plain-text draft: one paragraph, `\n` as hard breaks. */
export function chatInputDoc(text: string): ChatInputDoc {
  const content: ChatInputDoc["content"][0]["content"] = [];
  text.split("\n").forEach((line, index) => {
    if (index > 0) content.push({ type: "hardBreak" });
    // ProseMirror rejects empty text nodes; an empty line is just the break.
    if (line !== "") content.push({ type: "text", text: line });
  });
  return { type: "doc", content: [{ type: "paragraph", content }] };
}

/** Block separator used when reading the draft back out of the editor. */
export const CHAT_INPUT_BLOCK_SEPARATOR = "\n";

/** The slice of a keyboard event the submit decision reads. */
export interface ChatInputKey {
  key: string;
  shiftKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  isComposing?: boolean;
}

export type ChatInputKeyAction = "submit" | "newline" | "swallow" | "pass";

/**
 * What a key does in the composer (AGT-049): an unmodified Enter submits,
 * Shift+Enter breaks the line, an Enter chord (Control/Command/Alt) does
 * nothing at all, and every other key is the editor's. Enter while an IME
 * composition is open belongs to the IME, never to submit.
 */
export function chatInputKeyAction(event: ChatInputKey): ChatInputKeyAction {
  if (event.key !== "Enter") return "pass";
  if (event.isComposing === true) return "pass";
  if (event.ctrlKey || event.metaKey || event.altKey) return "swallow";
  if (event.shiftKey) return "newline";
  return "submit";
}
