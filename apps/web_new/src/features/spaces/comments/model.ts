// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public comment row model (parity/specs/space.md §3.9–3.10, SPC-049–057).
// The space subtree reads EXTERNAL comments through its own public
// endpoint; these helpers keep the row's presentation decisions pure so
// the component only lays them out. Nothing here mutates a comment or
// knows about workspace scope.

import {
  spaceAssetReadPath,
  type SpaceBoardMember,
  type SpaceComment,
  type SpaceCommentReaction,
} from "@pidash/api-client";

import { isStoredHtmlEmptyIgnoring, MENTION_CONTENT_TAGS } from "../../../shared/editor/index.js";
import { spaceReactionEmoji, type SpaceReactionGroup, type SpaceReactionWrite } from "../engagement/model.js";

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

/** Status a freshly uploaded comment picture carries in the stored body. */
export const SPACE_COMMENT_IMAGE_UPLOADED_STATUS = "uploaded";

/** Whether `viewerId` authored this comment (SPC-055). */
export function isSpaceCommentAuthor(comment: Pick<SpaceComment, "actor">, viewerId: string | null): boolean {
  return viewerId !== null && comment.actor !== null && comment.actor === viewerId;
}

/**
 * Whether a row offers its owner menu (SPC-055): the visitor is signed
 * in, authored the comment, and reads outside an embedding frame. Other
 * visitors' rows and every framed row carry no menu at all.
 */
export function canShowSpaceCommentMenu(
  comment: Pick<SpaceComment, "actor">,
  viewerId: string | null,
  embedded: boolean
): boolean {
  if (embedded) return false;
  return isSpaceCommentAuthor(comment, viewerId);
}

/**
 * True when a comment body has neither words nor a stored content node
 * (SPC-053): an image-only note is still a post, and mentions stand in
 * for the name they carry.
 */
export function isSpaceCommentBodyEmpty(html: string): boolean {
  return isStoredHtmlEmptyIgnoring(html, MENTION_CONTENT_TAGS);
}

/** Form state a plain Enter keypress decides on (SPC-053). */
export interface SpaceCommentEnterState {
  /** A mention suggestion shows and Enter would pick it. */
  mentionOpen: boolean;
  /** A post/save or an upload is in flight. */
  busy: boolean;
  /** The draft holds neither words nor a stored content node. */
  empty: boolean;
}

/** What a plain Enter does: pick the mention, submit, or nothing. */
export type SpaceCommentEnterAction = "pick-mention" | "submit" | "none";

/** One Enter rule both public comment forms share (SPC-053/056). */
export function spaceCommentEnterAction(state: SpaceCommentEnterState): SpaceCommentEnterAction {
  if (state.mentionOpen) return "pick-mention";
  if (state.busy || state.empty) return "none";
  return "submit";
}

/**
 * Whether the editing surface swallows a plain Enter before it reaches
 * the form. A swallowed Enter inserts no newline: picks and no-ops
 * leave the draft byte-identical, and only a submit bubbles on.
 */
export function swallowSpaceCommentEnter(state: SpaceCommentEnterState): boolean {
  return spaceCommentEnterAction(state) !== "submit";
}

/** How many people one mention search may offer (SPC-054). */
export const SPACE_MENTION_LIMIT = 5;

/** One mention suggestion row the public @ popup renders. */
export interface SpaceMentionPerson {
  id: string;
  name: string;
  avatarUrl: string | null;
}

/** The person one public board member offers to the @ popup. */
export function spaceMentionPerson(member: SpaceBoardMember): SpaceMentionPerson {
  const avatar = member.member__avatar.trim();
  return { id: member.member, name: member.member__display_name, avatarUrl: avatar === "" ? null : avatar };
}

/**
 * Members whose display name contains the @-query, capped for the popup
 * (SPC-054). A bare trigger with no text offers nobody: the popup opens
 * only once at least one character follows the @.
 */
export function matchSpaceMentionMembers(members: readonly SpaceBoardMember[], query: string): SpaceMentionPerson[] {
  if (query === "") return [];
  const needle = query.toLowerCase();
  const matched: SpaceMentionPerson[] = [];
  for (const member of members) {
    if (matched.length >= SPACE_MENTION_LIMIT) break;
    if (member.member__display_name.toLowerCase().includes(needle)) {
      matched.push(spaceMentionPerson(member));
    }
  }
  return matched;
}

/** The reaction fields the public grouping reads off a comment row. */
export type SpaceCommentReactionRow = Pick<SpaceCommentReaction, "actor" | "reaction" | "display_name">;

/** Reactor name when the server row carries a blank display name. */
export const SPACE_UNKNOWN_COMMENT_REACTOR = "Unknown member";

/**
 * Group a comment's flat reaction rows by their API code in first-seen
 * order (SPC-057), which keeps the row stable while a group beside it
 * changes. The flat shape is read directly (the NEWFRONT-466 fix): the
 * payload never sends the nested actor the issue endpoints use.
 */
export function groupSpaceCommentReactions(
  rows: readonly SpaceCommentReactionRow[],
  viewerId: string | null | undefined
): SpaceReactionGroup[] {
  const byCode = new Map<string, { names: SpaceReactionGroup["names"]; reacted: boolean }>();
  for (const row of rows) {
    const mine = viewerId !== null && viewerId !== undefined && row.actor === viewerId;
    const stored = row.display_name.trim();
    const name = stored === "" ? SPACE_UNKNOWN_COMMENT_REACTOR : stored;
    const existing = byCode.get(row.reaction);
    if (existing === undefined) {
      byCode.set(row.reaction, { names: [{ name, mine }], reacted: mine });
    } else {
      existing.names.push({ name, mine });
      existing.reacted = existing.reacted || mine;
    }
  }
  return Array.from(byCode, ([code, group]) => ({
    code,
    emoji: spaceReactionEmoji(code),
    count: group.names.length,
    names: group.names,
    reacted: group.reacted,
  }));
}

/** Whether the visitor already reacted to a comment with one code. */
export function viewerSpaceCommentReacted(
  rows: readonly SpaceCommentReactionRow[],
  viewerId: string | null | undefined,
  code: string
): boolean {
  if (viewerId === null || viewerId === undefined) return false;
  return rows.some((row) => row.actor === viewerId && row.reaction === code);
}

/**
 * The reaction rows a pending toggle overlays on the server rows
 * (SPC-057): the visitor's row for the code is gone and, for an add,
 * their new row is appended. Without a signed-in visitor there is no
 * overlay to paint.
 */
export function overlaySpaceCommentReaction(
  rows: readonly SpaceCommentReactionRow[],
  viewer: { id: string; name: string } | null,
  pending: SpaceReactionWrite | null
): SpaceCommentReactionRow[] {
  if (pending === null || viewer === null) return [...rows];
  const kept = rows.filter((row) => !(row.actor === viewer.id && row.reaction === pending.code));
  if (pending.action === "remove") return kept;
  return [...kept, { actor: viewer.id, reaction: pending.code, display_name: viewer.name }];
}

/**
 * Display address for a comment image the upload lifecycle confirmed:
 * the public anchor asset route against the client's origin (SPC-053).
 */
export function spaceCommentImageSrc(baseUrl: string, anchor: string, assetId: string): string {
  const path = spaceAssetReadPath(anchor, assetId);
  if (baseUrl === "") return path;
  return `${baseUrl.replace(/\/+$/, "")}${path}`;
}
