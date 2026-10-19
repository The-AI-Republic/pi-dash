// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reaction rules (parity/specs/issues.md §16, ISS-203/204). The API keys
// a reaction by its decimal code points joined with "-" ("1f44d-1f3fd");
// the UI shows the character. These helpers convert both ways, group the
// flat server rows into one chip per emoji, and decide the toggle the
// next click performs.

/** Off-the-shelf shelf the picker offers; the API accepts any code point key. */
export const REACTION_CHOICES: readonly string[] = [
  "👍",
  "👎",
  "😀",
  "🎉",
  "❤️",
  "🚀",
  "👀",
  "🙏",
  "🔥",
  "✅",
  "💡",
  "🙌",
  "🤔",
  "😮",
  "😢",
  "🫡",
];

/** The API key for one emoji character ("👍" -> "1f44d"). */
export function emojiToReaction(emoji: string): string {
  return Array.from(emoji)
    .map((character) => character.codePointAt(0))
    .filter((point): point is number => point !== undefined)
    .map((point) => point.toString(16))
    .join("-");
}

/** The character for one API key; an unknown key shows the key itself. */
export function reactionToEmoji(reaction: string): string {
  const points = reaction
    .split("-")
    .map((part) => Number.parseInt(part, 16))
    .filter((point) => Number.isInteger(point) && point >= 0);
  if (points.length === 0 || points.some((point) => point > 0x10ffff)) return reaction;
  return String.fromCodePoint(...points);
}

/** Names of the reactors in row order. */
export interface ReactionRow {
  readonly id: string;
  readonly actor: string;
  readonly reaction: string;
  readonly displayName: string;
}

/**
 * Called with the reactor's display name. Issue rows carry a full user
 * projection; comment rows carry only `display_name`.
 */
export function issueReactionRows(
  rows: readonly {
    id: string;
    actor: string;
    reaction: string;
    actor_detail: { display_name: string } | null;
  }[]
): ReactionRow[] {
  return rows.map((row) => ({
    id: row.id,
    actor: row.actor,
    reaction: row.reaction,
    displayName: row.actor_detail?.display_name ?? "Unknown member",
  }));
}

export function commentReactionRows(
  rows: readonly { id: string; actor: string; reaction: string; display_name: string }[]
): ReactionRow[] {
  return rows.map((row) => ({
    id: row.id,
    actor: row.actor,
    reaction: row.reaction,
    displayName: row.display_name.trim() === "" ? "Unknown member" : row.display_name.trim(),
  }));
}

/** One chip: a reaction key, its character, everyone who chose it. */
export interface ReactionChip {
  readonly code: string;
  readonly emoji: string;
  readonly count: number;
  readonly names: readonly string[];
  /** The signed-in viewer is among the reactors. */
  readonly reacted: boolean;
}

/**
 * Group flat rows into chips in first-seen order, which keeps the chip
 * row stable while a reaction is added or removed next to it.
 */
export function groupReactions(rows: readonly ReactionRow[], viewerId: string | null): ReactionChip[] {
  const byCode = new Map<string, { names: string[]; reacted: boolean }>();
  for (const row of rows) {
    const existing = byCode.get(row.reaction);
    const viewerReacted = viewerId !== null && row.actor === viewerId;
    if (existing === undefined) {
      byCode.set(row.reaction, { names: [row.displayName], reacted: viewerReacted });
    } else {
      existing.names.push(row.displayName);
      existing.reacted = existing.reacted || viewerReacted;
    }
  }
  return Array.from(byCode, ([code, group]) => ({
    code,
    emoji: reactionToEmoji(code),
    count: group.names.length,
    names: group.names,
    reacted: group.reacted,
  }));
}

/** Tooltip copy for one chip: who reacted, capped with an overflow count. */
export function chipTooltip(chip: Pick<ReactionChip, "emoji" | "names">, maxNames = 5): string {
  if (chip.names.length === 0) return chip.emoji;
  const shown = chip.names.slice(0, maxNames);
  const rest = chip.names.length - shown.length;
  return rest > 0 ? `${chip.emoji} ${shown.join(", ")} and ${rest} more` : `${chip.emoji} ${shown.join(", ")}`;
}

/** Whether the next activation of a chip adds or removes the viewer's row. */
export function reactionAction(chip: Pick<ReactionChip, "reacted">): "add" | "remove" {
  return chip.reacted ? "remove" : "add";
}

/** Whether the viewer already picked this reaction key. */
export function viewerHasReaction(rows: readonly ReactionRow[], viewerId: string | null, code: string): boolean {
  return viewerId !== null && rows.some((row) => row.actor === viewerId && row.reaction === code);
}
