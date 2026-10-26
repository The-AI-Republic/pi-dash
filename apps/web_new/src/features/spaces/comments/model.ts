// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public comment row model (parity/specs/space.md §3.9, SPC-049–052).
// The space subtree reads EXTERNAL comments through its own public
// endpoint; these helpers keep the row's presentation decisions pure so
// the component only lays them out. Nothing here mutates a comment or
// knows about workspace scope.

import type { SpaceComment } from "@pidash/api-client";

/** Server marker that folds a comment until the visitor expands it. */
export const SPACE_COMMENT_FOLD_LABEL = "fold";

/** Fallback label for an agent row whose stored speaker label is blank. */
export const SPACE_GENERIC_AGENT_LABEL = "Agent";

/** Display identity the public row derives from `actor_detail`. */
export interface SpaceCommentAuthor {
  name: string;
  initial: string;
  avatar: string | undefined;
  isBot: boolean;
}

/** Whether a comment carries the server's folded marker (SPC-051). */
export function isSpaceCommentFolded(comment: Pick<SpaceComment, "labels">): boolean {
  return comment.labels.includes(SPACE_COMMENT_FOLD_LABEL);
}

/**
 * Author line for a row. A bot's name is the account first name followed by
 * " Bot" as part of the identity itself (SPC-050), never a separate badge.
 * A comment with no actor details reads as a generic public author so the
 * row still has a stable identity.
 */
export function spaceCommentAuthor(comment: Pick<SpaceComment, "actor_detail">): SpaceCommentAuthor {
  const detail = comment.actor_detail;
  if (detail === null) return { name: "Unknown author", initial: "?", avatar: undefined, isBot: false };
  const first = detail.first_name.trim();
  const display = detail.display_name.trim();
  const name = detail.is_bot
    ? first === ""
      ? "Bot"
      : `${first} Bot`
    : display === ""
      ? first === ""
        ? "Unknown author"
        : first
      : display;
  const initial = name.charAt(0).toUpperCase();
  const avatar = detail.avatar_url?.trim();
  return { name, initial, avatar: avatar === undefined || avatar === "" ? undefined : avatar, isBot: detail.is_bot };
}

/**
 * Own-line speaker label for a machine-authored row (SPC-052). Only the
 * exact `agent` type qualifies; unknown values are tolerated as "not an
 * agent". A blank label falls back to the generic agent label. The value is
 * returned without the colon the row appends.
 */
export function spaceCommentSpeakerLabel(comment: Pick<SpaceComment, "speaker_type" | "speaker_label">): string | null {
  if (comment.speaker_type !== "agent") return null;
  const label = comment.speaker_label.trim();
  return label === "" ? SPACE_GENERIC_AGENT_LABEL : label;
}

/** Body HTML to render, falling back to the server's stripped text. */
export function spaceCommentBodyHtml(comment: Pick<SpaceComment, "comment_html" | "comment_stripped">): string {
  return comment.comment_html.trim() === "" ? `<p>${escapeHtml(comment.comment_stripped)}</p>` : comment.comment_html;
}

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}
