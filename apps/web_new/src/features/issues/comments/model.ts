// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Comment card rules (parity/specs/issues.md §16, ISS-197–202). Pure
// helpers the composer, the card menu and the deep-link highlight share:
// for whom a comment is authored, which menu entries that viewer gets,
// how the fold marker rides in the comment's labels, the display name the
// header shows, and the deep link a copied comment points at.

import { COMMENT_FOLD_LABEL, type IssueComment } from "@pidash/api-client";

import { isStoredHtmlEmpty } from "../../../shared/editor/index.js";

/** Card header name. Agent messages carry their own speaker label. */
export const AGENT_SPEAKER_FALLBACK = "AI Agent";
export const UNKNOWN_AUTHOR = "Unknown author";

/** Whether `viewerId` may edit or delete this comment (ISS-198/199). */
export function isCommentAuthor(comment: Pick<IssueComment, "actor">, viewerId: string | null): boolean {
  return viewerId !== null && comment.actor === viewerId;
}

/** Fold/unfold marker persisted as a comment label (ISS-201). */
export function isCommentFolded(comment: Pick<IssueComment, "labels">): boolean {
  return comment.labels.includes(COMMENT_FOLD_LABEL);
}

/** Labels with the fold marker flipped to `folded`. */
export function withFold(comment: Pick<IssueComment, "labels">, folded: boolean): string[] {
  const rest = comment.labels.filter((label) => label !== COMMENT_FOLD_LABEL);
  return folded ? [...rest, COMMENT_FOLD_LABEL] : rest;
}

/** Access a toggle flips to: internal comments become external and back. */
export function toggledAccess(comment: Pick<IssueComment, "access">): "INTERNAL" | "EXTERNAL" {
  return comment.access === "INTERNAL" ? "EXTERNAL" : "INTERNAL";
}

/**
 * Menu entry labels for one comment. Edit and delete are author-only even
 * for admins; fold is open to every viewer (folding noisy agent messages
 * is the main use); the access switch and the link live behind flags the
 * caller derives from the project and the surface.
 */
export interface CommentMenuFlags {
  viewerId: string | null;
  /** Project is anchored/published, so the access switch applies (ISS-200). */
  accessToggle: boolean;
  /** Surface offers deep links; intake items do not (ISS-202). */
  copyLink: boolean;
}

export interface CommentMenuEntry {
  key: "edit" | "copy-link" | "access" | "fold" | "delete";
  label: string;
}

export function commentMenuEntries(
  comment: Pick<IssueComment, "actor" | "labels" | "access">,
  flags: CommentMenuFlags
): CommentMenuEntry[] {
  const author = isCommentAuthor(comment, flags.viewerId);
  return [
    ...(author ? [{ key: "edit" as const, label: "Edit" }] : []),
    ...(flags.copyLink ? [{ key: "copy-link" as const, label: "Copy link" }] : []),
    ...(flags.accessToggle
      ? [
          {
            key: "access" as const,
            label: toggledAccess(comment) === "EXTERNAL" ? "Switch to public comment" : "Switch to private comment",
          },
        ]
      : []),
    {
      key: "fold",
      label: isCommentFolded(comment) ? "Unfold comment" : "Fold comment",
    },
    ...(author ? [{ key: "delete" as const, label: "Delete" }] : []),
  ];
}

/** Header name: speaker label for agents, profile name otherwise. */
export function commentAuthorName(
  comment: Pick<IssueComment, "speaker_type" | "speaker_label" | "actor_detail">
): string {
  if (comment.speaker_type === "agent") return comment.speaker_label.trim() || AGENT_SPEAKER_FALLBACK;
  return comment.actor_detail?.display_name ?? UNKNOWN_AUTHOR;
}

/** Avatar image for a comment author, when the server carries one. */
export function commentAuthorAvatar(comment: Pick<IssueComment, "actor_detail">): string | undefined {
  return comment.actor_detail?.avatar_url ?? undefined;
}

/** Deep link a comment's "Copy link" entry copies. */
export function commentDeepLink(issueUrl: string, commentId: string): string {
  return `${issueUrl}#comment-${commentId}`;
}

/** True for stored HTML that holds no visible words. */
export function isCommentBodyEmpty(html: string): boolean {
  return isStoredHtmlEmpty(html);
}

/** A change the card made to the comment, as the cache paints it. */
export interface CommentPatch {
  labels?: string[];
  access?: IssueComment["access"];
  comment_html?: string;
  edited_at?: string | null;
}

/** Next comment row with a patch painted on. */
export function applyCommentPatch(comment: IssueComment, patch: CommentPatch): IssueComment {
  return { ...comment, ...patch };
}

/** Next comment list with one patched row; unknown ids pass through. */
export function patchCommentList(
  comments: readonly IssueComment[],
  commentId: string,
  patch: CommentPatch
): IssueComment[] {
  return comments.map((comment) => (comment.id === commentId ? applyCommentPatch(comment, patch) : comment));
}
