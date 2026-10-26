// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public engagement rules (parity/specs/space.md §3.8, SPC-047/048). Pure
// helpers only: vote tallies, reaction groups, toggle decisions and the
// optimistic rows a write applies before the server answers. Both surfaces
// (the overlay section and the board-card footer) read these, so the two
// can never disagree about a count, a name or the visitor's own mark.

import type {
  SpaceBoardIssue,
  SpaceEngagementActor,
  SpaceIssueReactionItem,
  SpaceIssueVoteItem,
} from "@pidash/api-client";

/** One named voter; the visitor's own row carries the mark. */
export interface SpaceVoter {
  name: string;
  mine: boolean;
}

/** One vote side: its count, its voters in row order, the own mark. */
export interface SpaceVoteSideState {
  count: number;
  voters: SpaceVoter[];
  mine: boolean;
}

/** Both sides plus the visitor's own vote, if any. */
export interface SpaceVoteTally {
  up: SpaceVoteSideState;
  down: SpaceVoteSideState;
  mine: 1 | -1 | null;
}

function emptySide(): SpaceVoteSideState {
  return { count: 0, voters: [], mine: false };
}

/**
 * Split the embedded vote rows into the up/down sides (SPC-047). Names
 * stay in row order; rows with any other value are not votes and are
 * ignored rather than shown on either side.
 */
export function tallySpaceVotes(
  items: readonly SpaceIssueVoteItem[],
  viewerId: string | null | undefined
): SpaceVoteTally {
  const up = emptySide();
  const down = emptySide();
  for (const item of items) {
    const side = item.vote === 1 ? up : item.vote === -1 ? down : null;
    if (side === null) continue;
    const mine = viewerId !== null && viewerId !== undefined && item.actor_details.id === viewerId;
    side.count += 1;
    side.voters.push({ name: item.actor_details.display_name, mine });
    side.mine = side.mine || mine;
  }
  return { up, down, mine: up.mine ? 1 : down.mine ? -1 : null };
}

/** The visitor's own vote side, if they voted. */
export function currentSpaceVote(
  items: readonly SpaceIssueVoteItem[],
  viewerId: string | null | undefined
): 1 | -1 | null {
  return tallySpaceVotes(items, viewerId).mine;
}

export type SpaceVoteWrite = { action: "remove" } | { action: "set"; value: 1 | -1 };

/**
 * The write one activation performs (SPC-047): the same side again
 * removes the single vote, the other side (or no vote) sets it.
 */
export function nextSpaceVote(mine: 1 | -1 | null, clicked: 1 | -1): SpaceVoteWrite {
  if (mine === clicked) return { action: "remove" };
  return { action: "set", value: clicked };
}

/**
 * The vote rows after an optimistic write: the visitor's rows are gone
 * and, for a set, their new row is appended (SPC-047).
 */
export function optimisticSpaceVotes(
  items: readonly SpaceIssueVoteItem[],
  actor: SpaceEngagementActor,
  write: SpaceVoteWrite
): SpaceIssueVoteItem[] {
  const kept = items.filter((item) => item.actor_details.id !== actor.id);
  if (write.action === "remove") return kept;
  return [...kept, { vote: write.value, actor_details: actor }];
}

/** One reaction group: an emoji, its count, its reactors in row order. */
export interface SpaceReactionGroup {
  code: string;
  emoji: string;
  count: number;
  names: SpaceVoter[];
  reacted: boolean;
}

/** The character for one API reaction key; an unknown key shows itself. */
export function spaceReactionEmoji(code: string): string {
  const parts = code.split("-");
  if (parts.some((part) => !/^\d+$/.test(part))) return code;
  const points = parts.map((part) => Number(part));
  if (points.some((point) => point > 0x10ffff)) return code;
  return String.fromCodePoint(...points);
}

/** The API key for one emoji ("🎉" -> "127881"). */
export function spaceEmojiToReaction(emoji: string): string {
  return Array.from(emoji)
    .map((character) => character.codePointAt(0))
    .filter((point): point is number => point !== undefined)
    .map((point) => String(point))
    .join("-");
}

/**
 * Group the embedded reaction rows by their API code in first-seen order
 * (SPC-048), which keeps the row stable while a group beside it changes.
 */
export function groupSpaceReactions(
  items: readonly SpaceIssueReactionItem[],
  viewerId: string | null | undefined
): SpaceReactionGroup[] {
  const byCode = new Map<string, { names: SpaceVoter[]; reacted: boolean }>();
  for (const item of items) {
    const mine = viewerId !== null && viewerId !== undefined && item.actor_details.id === viewerId;
    const existing = byCode.get(item.reaction);
    if (existing === undefined) {
      byCode.set(item.reaction, { names: [{ name: item.actor_details.display_name, mine }], reacted: mine });
    } else {
      existing.names.push({ name: item.actor_details.display_name, mine });
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

/** Whether the visitor already reacted with one code. */
export function viewerSpaceReacted(
  items: readonly SpaceIssueReactionItem[],
  viewerId: string | null | undefined,
  code: string
): boolean {
  if (viewerId === null || viewerId === undefined) return false;
  return items.some((item) => item.actor_details.id === viewerId && item.reaction === code);
}

export type SpaceReactionWrite = { action: "add"; code: string } | { action: "remove"; code: string };

/**
 * The write one activation performs (SPC-048): re-clicking the visitor's
 * own group removes it, any other activation adds theirs to that group.
 */
export function nextSpaceReaction(reacted: boolean, code: string): SpaceReactionWrite {
  return reacted ? { action: "remove", code } : { action: "add", code };
}

/**
 * The reaction rows after an optimistic write: the visitor's row for the
 * code is gone and, for an add, their new row is appended (SPC-048).
 */
export function optimisticSpaceReactions(
  items: readonly SpaceIssueReactionItem[],
  actor: SpaceEngagementActor,
  write: SpaceReactionWrite
): SpaceIssueReactionItem[] {
  const kept = items.filter((item) => !(item.actor_details.id === actor.id && item.reaction === write.code));
  if (write.action === "remove") return kept;
  return [...kept, { reaction: write.code, actor_details: actor }];
}

/** How many reactor names a hint shows before the overflow note. */
export const SPACE_HINT_NAMES = 5;

export interface SpaceHintWords {
  /** The visitor's own mark, e.g. "you" in "Ada (you)". */
  you: string;
  /** The short note a side with no voters shows. */
  empty: string;
}

function hintName(voter: SpaceVoter, you: string): string {
  return voter.mine ? `${voter.name} (${you})` : voter.name;
}

/**
 * The hover/focus hint for one vote side (SPC-047): every voter in order
 * with the visitor's own marked, or the short empty note.
 */
export function spaceVoteHint(side: SpaceVoteSideState, words: SpaceHintWords): string {
  if (side.voters.length === 0) return words.empty;
  return side.voters.map((voter) => hintName(voter, words.you)).join(", ");
}

/**
 * The hover hint for one reaction group (SPC-048): the reactors in order
 * with the visitor's own marked, truncated past a handful with the
 * overflow count.
 */
export function spaceReactionHint(group: Pick<SpaceReactionGroup, "names">, you: string): string {
  const shown = group.names.slice(0, SPACE_HINT_NAMES).map((voter) => hintName(voter, you));
  const rest = group.names.length - shown.length;
  return rest > 0 ? `${shown.join(", ")} +${rest}` : shown.join(", ");
}

/** The engagement fields a toggle reads off an issue row or detail. */
export type SpaceEngageableIssue = Pick<SpaceBoardIssue, "id" | "vote_items" | "reaction_items">;
