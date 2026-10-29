// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Comment card rules (parity/specs/issues.md §16, ISS-197–202). Pure
// helpers the composer, the card menu and the deep-link highlight share:
// for whom a comment is authored, which menu entries that viewer gets,
// how the fold marker rides in the comment's labels, the display name the
// header shows, the deep link a copied comment points at, the asset ids
// an edit duplicates (CMT-009), and the Enter key decision (CMT-011).

import { COMMENT_FOLD_LABEL, type IssueComment } from "@pidash/api-client";

import { isStoredHtmlEmptyIgnoring } from "../../../shared/editor/index.js";

/**
 * Tags that count as a comment's content even without words (§16,
 * ISS-197): an image-only note is still a post, and mentions stand in for
 * the name they carry.
 */
export const COMMENT_CONTENT_TAGS = ["img", "image-component", "mention-component"] as const;

/** Card header name. Agent messages carry their own speaker label. */
export const AGENT_SPEAKER_FALLBACK = "AI Agent";
export const UNKNOWN_AUTHOR = "Unknown author";
/** Suffix marking a bot account's display name (CMT-012). */
export const BOT_AUTHOR_SUFFIX = "Bot";

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

/**
 * Header name: speaker label for agents, a bot-marked profile name for bot
 * accounts, the profile name for humans. A bot whose display name already
 * reads as a bot keeps it as stored, as does a human with "bot" in theirs.
 */
export function commentAuthorName(
  comment: Pick<IssueComment, "speaker_type" | "speaker_label" | "actor_detail">
): string {
  if (comment.speaker_type === "agent") return comment.speaker_label.trim() || AGENT_SPEAKER_FALLBACK;
  const name = comment.actor_detail?.display_name ?? UNKNOWN_AUTHOR;
  if (comment.actor_detail?.is_bot === true && !/bot/i.test(name)) return `${name} ${BOT_AUTHOR_SUFFIX}`;
  return name;
}

/** Avatar image for a comment author, when the server carries one. */
export function commentAuthorAvatar(comment: Pick<IssueComment, "actor_detail">): string | undefined {
  return comment.actor_detail?.avatar_url ?? undefined;
}

/** Deep link a comment's "Copy link" entry copies. */
export function commentDeepLink(issueUrl: string, commentId: string): string {
  return `${issueUrl}#comment-${commentId}`;
}

/** True when a comment body has neither words nor image/mention markup. */
export function isCommentBodyEmpty(html: string): boolean {
  return isStoredHtmlEmptyIgnoring(html, COMMENT_CONTENT_TAGS);
}

/** Status a freshly uploaded comment picture carries in the stored body. */
export const COMMENT_IMAGE_UPLOADED_STATUS = "uploaded";

/**
 * Asset id a comment image node points at (CMT-009). Stored bodies keep
 * either the bare asset id or the served asset path, app-relative or
 * absolute; anything else (external pictures, data URLs) has no asset
 * to duplicate and answers null.
 */
export function commentImageAssetId(src: string): string | null {
  const value = src.trim();
  if (value === "") return null;
  const marker = "/api/assets/v2/";
  const at = value.indexOf(marker);
  if (at !== -1) {
    const tail = value
      .slice(at + marker.length)
      .split("/")
      .filter((part) => part !== "");
    const id = tail[tail.length - 1];
    return id === undefined ? null : id;
  }
  if (value.includes("/") || value.includes(":") || /\s/.test(value)) return null;
  return value;
}

/** Asset ids a comment body embeds, in document order without repeats. */
export function commentBodyImageAssetIds(html: string): string[] {
  if (typeof globalThis.DOMParser === "undefined") return [];
  const document = new DOMParser().parseFromString(html, "text/html");
  const ids: string[] = [];
  for (const element of Array.from(document.querySelectorAll("img, image-component"))) {
    const id = commentImageAssetId(element.getAttribute("src") ?? "");
    if (id !== null && !ids.includes(id)) ids.push(id);
  }
  return ids;
}

/**
 * Next comment body with duplicated assets rebound (CMT-009). Every
 * image node whose asset id sits in `mapping` points at its duplicate
 * instead; anything else passes through untouched. An empty mapping
 * returns the body untouched, never renormalized.
 */
export function rewriteCommentImageSrcs(html: string, mapping: ReadonlyMap<string, string>): string {
  if (mapping.size === 0 || typeof globalThis.DOMParser === "undefined") return html;
  const document = new DOMParser().parseFromString(html, "text/html");
  for (const element of Array.from(document.querySelectorAll("img, image-component"))) {
    const id = commentImageAssetId(element.getAttribute("src") ?? "");
    if (id === null) continue;
    const duplicate = mapping.get(id);
    if (duplicate !== undefined) element.setAttribute("src", duplicate);
  }
  return document.body.innerHTML;
}

/**
 * Whether a draft is empty, asking the React snapshot and the live
 * editing surface (ISS-197). A keystroke reaches the editor before React
 * renders its output, so an Enter in that gap still reads an empty
 * snapshot; the live document already holds the typed words and must win.
 */
export function isCommentDraftEmpty(snapshotEmpty: boolean, liveHtml: string | null): boolean {
  if (!snapshotEmpty) return false;
  return liveHtml === null || isCommentBodyEmpty(liveHtml);
}

/** Form state a plain Enter keypress decides on (CMT-011). */
export interface CommentEnterState {
  /** A mention suggestion shows and Enter would pick it. */
  mentionOpen: boolean;
  /** A post/save or an upload is in flight. */
  busy: boolean;
  /** The draft holds neither words nor a stored content node. */
  empty: boolean;
}

/** What a plain Enter does: pick the mention, submit, or nothing. */
export type CommentEnterAction = "pick-mention" | "submit" | "none";

/** One Enter rule both comment forms share (CMT-011). */
export function commentEnterAction(state: CommentEnterState): CommentEnterAction {
  if (state.mentionOpen) return "pick-mention";
  if (state.busy || state.empty) return "none";
  return "submit";
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
