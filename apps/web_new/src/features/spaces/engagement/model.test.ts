// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceEngagementActor, SpaceIssueReactionItem, SpaceIssueVoteItem } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  currentSpaceVote,
  groupSpaceReactions,
  nextSpaceReaction,
  nextSpaceVote,
  optimisticSpaceReactions,
  optimisticSpaceVotes,
  spaceEmojiToReaction,
  spaceReactionEmoji,
  spaceReactionHint,
  spaceVoteHint,
  tallySpaceVotes,
  viewerSpaceReacted,
} from "./model.js";

const ADA = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const BO = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

function actor(id: string, displayName: string): SpaceEngagementActor {
  return { id, display_name: displayName, first_name: displayName, last_name: "", avatar: "", avatar_url: null };
}

function vote(value: number, id: string, name: string): SpaceIssueVoteItem {
  return { vote: value, actor_details: actor(id, name) };
}

function reaction(code: string, id: string, name: string): SpaceIssueReactionItem {
  return { reaction: code, actor_details: actor(id, name) };
}

const WORDS = { you: "you", empty: "No votes yet" };

describe("tallySpaceVotes", () => {
  it("splits rows into sides with names in row order and the own mark", () => {
    const tally = tallySpaceVotes([vote(1, ADA, "Ada"), vote(-1, BO, "Bo"), vote(1, BO, "Bo")], BO);
    expect(tally.up).toEqual({
      count: 2,
      voters: [
        { name: "Ada", mine: false },
        { name: "Bo", mine: true },
      ],
      mine: true,
    });
    expect(tally.down).toEqual({ count: 1, voters: [{ name: "Bo", mine: true }], mine: true });
    expect(tally.mine).toBe(1);
  });

  it("reads no own mark without a viewer", () => {
    const tally = tallySpaceVotes([vote(1, ADA, "Ada")], null);
    expect(tally.mine).toBeNull();
    expect(tally.up.voters).toEqual([{ name: "Ada", mine: false }]);
    expect(tally.down).toEqual({ count: 0, voters: [], mine: false });
  });

  it("reads no own mark while the viewer is still loading", () => {
    expect(tallySpaceVotes([vote(-1, ADA, "Ada")], undefined).mine).toBeNull();
  });

  it("ignores rows with no vote side", () => {
    const tally = tallySpaceVotes([vote(0, ADA, "Ada"), vote(2, BO, "Bo")], ADA);
    expect(tally.up.count).toBe(0);
    expect(tally.down.count).toBe(0);
    expect(tally.mine).toBeNull();
  });

  it("tallies an empty row list", () => {
    expect(tallySpaceVotes([], ADA)).toEqual({
      up: { count: 0, voters: [], mine: false },
      down: { count: 0, voters: [], mine: false },
      mine: null,
    });
  });
});

describe("currentSpaceVote", () => {
  it("names the visitor's own side", () => {
    expect(currentSpaceVote([vote(-1, ADA, "Ada")], ADA)).toBe(-1);
    expect(currentSpaceVote([vote(1, ADA, "Ada")], BO)).toBeNull();
  });
});

describe("nextSpaceVote", () => {
  it("removes on the same side and sets otherwise", () => {
    expect(nextSpaceVote(1, 1)).toEqual({ action: "remove" });
    expect(nextSpaceVote(-1, -1)).toEqual({ action: "remove" });
    expect(nextSpaceVote(1, -1)).toEqual({ action: "set", value: -1 });
    expect(nextSpaceVote(null, 1)).toEqual({ action: "set", value: 1 });
  });
});

describe("optimisticSpaceVotes", () => {
  it("drops the visitor's rows on remove and keeps everyone else's", () => {
    const rows = [vote(1, ADA, "Ada"), vote(-1, ADA, "Ada"), vote(1, BO, "Bo")];
    expect(optimisticSpaceVotes(rows, actor(ADA, "Ada"), { action: "remove" })).toEqual([vote(1, BO, "Bo")]);
  });

  it("moves the visitor's single vote on set", () => {
    const rows = [vote(-1, ADA, "Ada"), vote(1, BO, "Bo")];
    expect(optimisticSpaceVotes(rows, actor(ADA, "Ada"), { action: "set", value: 1 })).toEqual([
      vote(1, BO, "Bo"),
      vote(1, ADA, "Ada"),
    ]);
  });
});

describe("spaceReactionEmoji", () => {
  it("renders code points and echoes unknown keys", () => {
    expect(spaceReactionEmoji("127881")).toBe("🎉");
    expect(spaceReactionEmoji("128077-127995")).toBe("👍🏻");
    expect(spaceReactionEmoji("not-a-code")).toBe("not-a-code");
    expect(spaceReactionEmoji("1114112")).toBe("1114112");
    expect(spaceReactionEmoji("12x-34")).toBe("12x-34");
  });
});

describe("spaceEmojiToReaction", () => {
  it("keys an emoji by its decimal code points", () => {
    expect(spaceEmojiToReaction("🎉")).toBe("127881");
    expect(spaceEmojiToReaction("👍")).toBe("128077");
  });
});

describe("groupSpaceReactions", () => {
  it("groups by code in first-seen order with the own mark", () => {
    const groups = groupSpaceReactions(
      [reaction("127881", ADA, "Ada"), reaction("128077", BO, "Bo"), reaction("127881", BO, "Bo")],
      BO
    );
    expect(groups).toEqual([
      {
        code: "127881",
        emoji: "🎉",
        count: 2,
        names: [
          { name: "Ada", mine: false },
          { name: "Bo", mine: true },
        ],
        reacted: true,
      },
      {
        code: "128077",
        emoji: "👍",
        count: 1,
        names: [{ name: "Bo", mine: true }],
        reacted: true,
      },
    ]);
  });

  it("marks nothing reacted without a viewer", () => {
    const groups = groupSpaceReactions([reaction("127881", ADA, "Ada")], null);
    expect(groups[0]).toMatchObject({ reacted: false, names: [{ name: "Ada", mine: false }] });
    expect(groupSpaceReactions([reaction("127881", ADA, "Ada")], undefined)[0]?.reacted).toBe(false);
  });

  it("groups nothing", () => {
    expect(groupSpaceReactions([], ADA)).toEqual([]);
  });
});

describe("viewerSpaceReacted", () => {
  it("matches the visitor's row for one code only", () => {
    const rows = [reaction("127881", ADA, "Ada")];
    expect(viewerSpaceReacted(rows, ADA, "127881")).toBe(true);
    expect(viewerSpaceReacted(rows, ADA, "128077")).toBe(false);
    expect(viewerSpaceReacted(rows, BO, "127881")).toBe(false);
    expect(viewerSpaceReacted(rows, null, "127881")).toBe(false);
    expect(viewerSpaceReacted(rows, undefined, "127881")).toBe(false);
  });
});

describe("nextSpaceReaction", () => {
  it("removes the own group and adds any other", () => {
    expect(nextSpaceReaction(true, "127881")).toEqual({ action: "remove", code: "127881" });
    expect(nextSpaceReaction(false, "127881")).toEqual({ action: "add", code: "127881" });
  });
});

describe("optimisticSpaceReactions", () => {
  it("drops only the visitor's row for the code on remove", () => {
    const rows = [reaction("127881", ADA, "Ada"), reaction("127881", BO, "Bo"), reaction("128077", ADA, "Ada")];
    expect(optimisticSpaceReactions(rows, actor(ADA, "Ada"), { action: "remove", code: "127881" })).toEqual([
      reaction("127881", BO, "Bo"),
      reaction("128077", ADA, "Ada"),
    ]);
  });

  it("appends the visitor's row on add", () => {
    const rows = [reaction("127881", BO, "Bo")];
    expect(optimisticSpaceReactions(rows, actor(ADA, "Ada"), { action: "add", code: "127881" })).toEqual([
      reaction("127881", BO, "Bo"),
      reaction("127881", ADA, "Ada"),
    ]);
  });
});

describe("spaceVoteHint", () => {
  it("names every voter with the visitor's own marked", () => {
    expect(
      spaceVoteHint(
        {
          count: 2,
          voters: [
            { name: "Ada", mine: false },
            { name: "Bo", mine: true },
          ],
          mine: true,
        },
        WORDS
      )
    ).toBe("Ada, Bo (you)");
  });

  it("shows the empty note with no voters", () => {
    expect(spaceVoteHint({ count: 0, voters: [], mine: false }, WORDS)).toBe("No votes yet");
  });
});

describe("spaceReactionHint", () => {
  it("names the reactors with the visitor's own marked", () => {
    expect(
      spaceReactionHint(
        {
          names: [
            { name: "Ada", mine: false },
            { name: "Bo", mine: true },
          ],
        },
        "you"
      )
    ).toBe("Ada, Bo (you)");
  });

  it("truncates past a handful with the overflow count", () => {
    const names = Array.from({ length: 7 }, (_, index) => ({ name: `Voter ${index + 1}`, mine: false }));
    expect(spaceReactionHint({ names }, "you")).toBe("Voter 1, Voter 2, Voter 3, Voter 4, Voter 5 +2");
  });
});
