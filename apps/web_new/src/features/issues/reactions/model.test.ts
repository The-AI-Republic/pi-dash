// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reaction rules (ISS-203/204): code-point keys, chip grouping, viewer
// highlight and the toggle a click performs.
import { describe, expect, it } from "vitest";

import {
  REACTION_CHOICES,
  chipTooltip,
  commentReactionRows,
  emojiToReaction,
  groupReactions,
  issueReactionRows,
  reactionAction,
  reactionToEmoji,
  viewerHasReaction,
} from "./model.js";

const ADA = "523e4567-e89b-12d3-a456-426614174004";
const BOB = "623e4567-e89b-12d3-a456-426614174005";

describe("reaction keys", () => {
  it("encodes an emoji as its decimal code points, the stored contract", () => {
    expect(emojiToReaction("👍")).toBe("128077");
    expect(emojiToReaction("🎉")).toBe("127881");
    expect(emojiToReaction("❤️")).toBe("10084-65039");
    expect(emojiToReaction("")).toBe("");
  });

  it("decodes a stored decimal key back to the character", () => {
    expect(reactionToEmoji("128077")).toBe("👍");
    expect(reactionToEmoji("10084-65039")).toBe("❤️");
  });

  it("shows an unknown key verbatim rather than throwing", () => {
    expect(reactionToEmoji("thumbsup")).toBe("thumbsup");
    expect(reactionToEmoji("1114112")).toBe("1114112");
    expect(reactionToEmoji("128077-x")).toBe("128077-x");
    expect(reactionToEmoji("-1")).toBe("-1");
    expect(reactionToEmoji("")).toBe("");
  });

  it("round-trips every offered choice", () => {
    for (const emoji of REACTION_CHOICES) expect(reactionToEmoji(emojiToReaction(emoji))).toBe(emoji);
  });
});

describe("reaction rows", () => {
  it("reads the reactor name from the issue row projection", () => {
    const rows = issueReactionRows([
      { id: "r1", actor: ADA, reaction: "128077", actor_detail: { display_name: "Ada L" } },
      { id: "r2", actor: BOB, reaction: "128077", actor_detail: null },
    ]);
    expect(rows.map((row) => row.displayName)).toEqual(["Ada L", "Unknown member"]);
  });

  it("reads the comment row name and replaces a blank one", () => {
    const rows = commentReactionRows([
      { id: "r1", actor: ADA, reaction: "128077", display_name: " Ada L " },
      { id: "r2", actor: BOB, reaction: "128077", display_name: "   " },
    ]);
    expect(rows.map((row) => row.displayName)).toEqual(["Ada L", "Unknown member"]);
  });
});

describe("chip grouping", () => {
  const rows = [
    { id: "r1", actor: ADA, reaction: "128077", displayName: "Ada L" },
    { id: "r2", actor: BOB, reaction: "128077", displayName: "Bob B" },
    { id: "r3", actor: BOB, reaction: "127881", displayName: "Bob B" },
  ];

  it("groups by key in first-seen order with a count and names", () => {
    const chips = groupReactions(rows, null);
    expect(chips.map((chip) => [chip.emoji, chip.count, [...chip.names]])).toEqual([
      ["👍", 2, ["Ada L", "Bob B"]],
      ["🎉", 1, ["Bob B"]],
    ]);
  });

  it("highlights a chip only when the viewer chose it", () => {
    expect(groupReactions(rows, ADA).map((chip) => chip.reacted)).toEqual([true, false]);
    expect(groupReactions(rows, "other").map((chip) => chip.reacted)).toEqual([false, false]);
    expect(groupReactions(rows, null).map((chip) => chip.reacted)).toEqual([false, false]);
  });

  it("knows whether the viewer already picked a key", () => {
    expect(viewerHasReaction(rows, ADA, "128077")).toBe(true);
    expect(viewerHasReaction(rows, ADA, "127881")).toBe(false);
    expect(viewerHasReaction(rows, null, "128077")).toBe(false);
  });
});

describe("chip presentation", () => {
  it("names the reactors and counts the overflow", () => {
    const chip = { emoji: "👍", names: ["A", "B", "C", "D", "E", "F"] };
    expect(chipTooltip(chip)).toBe("👍 A, B, C, D, E and 1 more");
    expect(chipTooltip({ emoji: "👍", names: ["A", "B"] })).toBe("👍 A, B");
    expect(chipTooltip({ emoji: "👍", names: [] })).toBe("👍");
  });

  it("removes when reacted and adds otherwise", () => {
    expect(reactionAction({ reacted: true })).toBe("remove");
    expect(reactionAction({ reacted: false })).toBe("add");
  });
});
