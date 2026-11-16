// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceBoardMember, SpaceComment } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  canShowSpaceCommentMenu,
  groupSpaceCommentReactions,
  isSpaceCommentAuthor,
  isSpaceCommentBodyEmpty,
  isSpaceCommentFolded,
  matchSpaceMentionMembers,
  overlaySpaceCommentReaction,
  SPACE_COMMENT_FOLD_LABEL,
  SPACE_COMMENT_IMAGE_UPLOADED_STATUS,
  SPACE_GENERIC_AGENT_LABEL,
  SPACE_MENTION_LIMIT,
  SPACE_UNKNOWN_COMMENT_REACTOR,
  spaceCommentAuthor,
  spaceCommentBodyHtml,
  spaceCommentEnterAction,
  spaceCommentImageSrc,
  spaceCommentSpeakerLabel,
  spaceMentionPerson,
  isSpaceCommentDraftEmpty,
  viewerSpaceCommentReacted,
  type SpaceCommentReactionRow,
} from "./model.js";

const ADA = "523e4567-e89b-12d3-a456-426614174004";

function comment(overrides: Partial<SpaceComment> = {}): SpaceComment {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    actor: ADA,
    actor_detail: {
      id: ADA,
      first_name: "Ada",
      last_name: "L",
      avatar: "",
      avatar_url: null,
      display_name: "Ada L",
      is_bot: false,
    },
    issue: "223e4567-e89b-12d3-a456-426614174001",
    project: "323e4567-e89b-12d3-a456-426614174002",
    workspace: "423e4567-e89b-12d3-a456-426614174003",
    comment_html: "<p>Hello</p>",
    comment_stripped: "Hello",
    comment_json: {},
    attachments: [],
    labels: [],
    access: "EXTERNAL",
    edited_at: null,
    comment_reactions: [],
    speaker_type: "human",
    speaker_label: "",
    speaker_agent_run_id: null,
    parent: null,
    external_source: null,
    external_id: null,
    is_synced: false,
    description: null,
    created_at: "2026-10-16T22:00:00Z",
    updated_at: "2026-10-16T22:00:00Z",
    created_by: ADA,
    updated_by: null,
    deleted_at: null,
    ...overrides,
  };
}

describe("public comment fold marker", () => {
  it("recognises only the server's fold label", () => {
    expect(SPACE_COMMENT_FOLD_LABEL).toBe("fold");
    expect(isSpaceCommentFolded(comment())).toBe(false);
    expect(isSpaceCommentFolded(comment({ labels: ["fold"] }))).toBe(true);
    expect(isSpaceCommentFolded(comment({ labels: ["Fold"] }))).toBe(false);
  });
});

describe("public comment authorship", () => {
  it("uses the display name and its first initial", () => {
    expect(spaceCommentAuthor(comment())).toEqual({
      name: "Ada L",
      initial: "A",
      avatar: undefined,
      isBot: false,
    });
  });

  it("falls back to the first name when no display name is stored", () => {
    expect(
      spaceCommentAuthor(comment({ actor_detail: { ...comment().actor_detail!, display_name: "  " } }))
    ).toMatchObject({ name: "Ada", initial: "A" });
  });

  it("suffixes a bot's first name and tolerates a nameless bot", () => {
    expect(spaceCommentAuthor(comment({ actor_detail: { ...comment().actor_detail!, is_bot: true } }))).toMatchObject({
      name: "Ada Bot",
      initial: "A",
      isBot: true,
    });
    expect(
      spaceCommentAuthor(
        comment({
          actor_detail: { ...comment().actor_detail!, first_name: " ", display_name: "Bot Name", is_bot: true },
        })
      )
    ).toMatchObject({ name: "Bot", initial: "B" });
  });

  it("gives a row without actor details a stable unknown identity", () => {
    expect(spaceCommentAuthor(comment({ actor_detail: null }))).toEqual({
      name: "Unknown author",
      initial: "?",
      avatar: undefined,
      isBot: false,
    });
    expect(
      spaceCommentAuthor(comment({ actor_detail: { ...comment().actor_detail!, first_name: "", display_name: "" } }))
    ).toMatchObject({ name: "Unknown author", initial: "U" });
  });

  it("uses a trimmed uploaded avatar and ignores a blank one", () => {
    expect(
      spaceCommentAuthor(
        comment({ actor_detail: { ...comment().actor_detail!, avatar_url: "  https://img.test/a.png  " } })
      ).avatar
    ).toBe("https://img.test/a.png");
  });
});

describe("public comment speaker labels", () => {
  it("shows a label only for agent-authored rows", () => {
    expect(spaceCommentSpeakerLabel(comment({ speaker_type: "agent", speaker_label: "Probe Agent" }))).toBe(
      "Probe Agent"
    );
    expect(spaceCommentSpeakerLabel(comment({ speaker_type: "human", speaker_label: "Ada" }))).toBeNull();
    expect(spaceCommentSpeakerLabel(comment({ speaker_type: "integration", speaker_label: "Slack" }))).toBeNull();
  });

  it("falls back to the generic agent label when the stored label is blank", () => {
    expect(spaceCommentSpeakerLabel(comment({ speaker_type: "agent", speaker_label: "  " }))).toBe(
      SPACE_GENERIC_AGENT_LABEL
    );
  });
});

describe("public comment bodies", () => {
  it("keeps stored rich HTML", () => {
    expect(spaceCommentBodyHtml(comment({ comment_html: "<p>Hello <strong>there</strong></p>" }))).toBe(
      "<p>Hello <strong>there</strong></p>"
    );
  });

  it("escapes stripped text when stored HTML is blank", () => {
    expect(
      spaceCommentBodyHtml(
        comment({ comment_html: " ", comment_stripped: `A & B <script>alert("x")</script> 'quote'` })
      )
    ).toBe("<p>A &amp; B &lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt; &#39;quote&#39;</p>");
  });
});

const GUEST = "623e4567-e89b-12d3-a456-426614174005";

describe("comment authorship", () => {
  it("matches the visitor against the stored author (SPC-055)", () => {
    expect(isSpaceCommentAuthor(comment({ actor: ADA }), ADA)).toBe(true);
    expect(isSpaceCommentAuthor(comment({ actor: ADA }), GUEST)).toBe(false);
    expect(isSpaceCommentAuthor(comment({ actor: ADA }), null)).toBe(false);
    expect(isSpaceCommentAuthor(comment({ actor: null }), ADA)).toBe(false);
  });

  it("offers the owner menu only to the signed-in author outside embeds (SPC-055)", () => {
    expect(canShowSpaceCommentMenu(comment({ actor: ADA }), ADA, false)).toBe(true);
    expect(canShowSpaceCommentMenu(comment({ actor: ADA }), ADA, true)).toBe(false);
    expect(canShowSpaceCommentMenu(comment({ actor: ADA }), null, false)).toBe(false);
    expect(canShowSpaceCommentMenu(comment({ actor: GUEST }), ADA, false)).toBe(false);
    expect(canShowSpaceCommentMenu(comment({ actor: null }), ADA, false)).toBe(false);
  });
});

describe("comment body emptiness", () => {
  it("treats bare paragraphs and whitespace as empty (SPC-053)", () => {
    expect(isSpaceCommentBodyEmpty("<p></p>")).toBe(true);
    expect(isSpaceCommentBodyEmpty("<p>   </p>")).toBe(true);
    expect(isSpaceCommentBodyEmpty("")).toBe(true);
  });

  it("counts words, images and mentions as content (SPC-053/054)", () => {
    expect(isSpaceCommentBodyEmpty("<p>Hello</p>")).toBe(false);
    expect(isSpaceCommentBodyEmpty('<p><img src="asset-1"></p>')).toBe(false);
    expect(isSpaceCommentBodyEmpty('<p><image-component src="asset-1"></image-component></p>')).toBe(false);
    expect(
      isSpaceCommentBodyEmpty(
        '<p><mention-component id="m-1" entity_name="user_mention" data-name="Ada"></mention-component></p>'
      )
    ).toBe(false);
  });
});

describe("comment Enter rule", () => {
  it("picks the open mention, submits a ready draft, and holds otherwise (SPC-053)", () => {
    expect(spaceCommentEnterAction({ mentionOpen: true, busy: false, empty: false })).toBe("pick-mention");
    expect(spaceCommentEnterAction({ mentionOpen: false, busy: false, empty: false })).toBe("submit");
    expect(spaceCommentEnterAction({ mentionOpen: false, busy: true, empty: false })).toBe("none");
    expect(spaceCommentEnterAction({ mentionOpen: false, busy: false, empty: true })).toBe("none");
  });

  it("treats a stale empty snapshot as filled once the live draft has words (ISS-197)", () => {
    expect(isSpaceCommentDraftEmpty(true, "<p>Fresh words</p>")).toBe(false);
    expect(isSpaceCommentDraftEmpty(true, '<p><img src="asset-1"></p>')).toBe(false);
    expect(isSpaceCommentDraftEmpty(true, "<p></p>")).toBe(true);
    expect(isSpaceCommentDraftEmpty(true, null)).toBe(true);
    expect(isSpaceCommentDraftEmpty(false, null)).toBe(false);
  });
});

function member(overrides: Partial<SpaceBoardMember> = {}): SpaceBoardMember {
  return {
    id: "723e4567-e89b-12d3-a456-426614174006",
    member: ADA,
    member__display_name: "Ada L",
    member__avatar: "",
    ...overrides,
  };
}

describe("mention people", () => {
  it("binds the person to the membership's user id (SPC-054)", () => {
    expect(spaceMentionPerson(member())).toMatchObject({ id: ADA, name: "Ada L", avatarUrl: null });
    expect(spaceMentionPerson(member({ member__avatar: "https://cdn.test/a.png" }))).toMatchObject({
      avatarUrl: "https://cdn.test/a.png",
    });
    expect(spaceMentionPerson(member({ member__avatar: "" }))).toMatchObject({ avatarUrl: null });
    expect(spaceMentionPerson(member({ member__avatar: "   " }))).toMatchObject({ avatarUrl: null });
  });

  it("stamps uploaded pictures with the stored status (SPC-053)", () => {
    expect(SPACE_COMMENT_IMAGE_UPLOADED_STATUS).toBe("uploaded");
  });

  it("matches members by name, case-insensitively, capped for the popup (SPC-054)", () => {
    const ada = member({ member__display_name: "Ada Lovelace" });
    const grace = member({ member__display_name: "Grace Hopper", member: GUEST });
    expect(matchSpaceMentionMembers([ada, grace], "")).toEqual([]);
    expect(matchSpaceMentionMembers([ada, grace], "ada")).toEqual([{ id: ADA, name: "Ada Lovelace", avatarUrl: null }]);
    expect(matchSpaceMentionMembers([ada, grace], "HOP")).toEqual([
      { id: GUEST, name: "Grace Hopper", avatarUrl: null },
    ]);
    expect(matchSpaceMentionMembers([ada, grace], "zzz")).toEqual([]);
    const crowd = Array.from({ length: SPACE_MENTION_LIMIT + 2 }, (_, index) =>
      member({ member__display_name: `Member ${index}`, member: `member-${index}` })
    );
    const matched = matchSpaceMentionMembers(crowd, "member");
    expect(matched).toHaveLength(SPACE_MENTION_LIMIT);
    expect(matched[0]).toMatchObject({ name: "Member 0" });
  });
});

function reactionRow(overrides: Partial<SpaceCommentReactionRow> = {}): SpaceCommentReactionRow {
  return { actor: ADA, reaction: "127881", display_name: "Ada L", ...overrides };
}

describe("comment reaction grouping", () => {
  it("groups flat rows by code in first-seen order (SPC-057)", () => {
    expect(groupSpaceCommentReactions([], ADA)).toEqual([]);
    const groups = groupSpaceCommentReactions(
      [
        reactionRow({ actor: GUEST, display_name: "Guest" }),
        reactionRow({ reaction: "128077", display_name: "Ada L" }),
        reactionRow({ actor: GUEST, reaction: "128077", display_name: "Guest" }),
      ],
      ADA
    );
    expect(groups.map((group) => group.code)).toEqual(["127881", "128077"]);
    expect(groups[0]).toMatchObject({ emoji: "🎉", count: 1, reacted: false });
    expect(groups[0]!.names).toEqual([{ name: "Guest", mine: false }]);
    expect(groups[1]).toMatchObject({ emoji: "👍", count: 2, reacted: true });
    expect(groups[1]!.names).toEqual([
      { name: "Ada L", mine: true },
      { name: "Guest", mine: false },
    ]);
  });

  it("marks the visitor's own pills on server-loaded rows (SPC-057)", () => {
    const rows = [reactionRow()];
    expect(groupSpaceCommentReactions(rows, ADA)[0]).toMatchObject({ reacted: true });
    expect(groupSpaceCommentReactions(rows, GUEST)[0]).toMatchObject({ reacted: false });
    expect(groupSpaceCommentReactions(rows, null)[0]).toMatchObject({ reacted: false });
    expect(groupSpaceCommentReactions(rows, undefined)[0]).toMatchObject({ reacted: false });
  });

  it("names blank reactors generically (SPC-057)", () => {
    const groups = groupSpaceCommentReactions([reactionRow({ display_name: "  " })], null);
    expect(groups[0]!.names).toEqual([{ name: SPACE_UNKNOWN_COMMENT_REACTOR, mine: false }]);
    expect(SPACE_UNKNOWN_COMMENT_REACTOR).toBe("Unknown member");
  });

  it("reads the visitor's own code from the flat rows (SPC-057)", () => {
    const rows = [reactionRow()];
    expect(viewerSpaceCommentReacted(rows, ADA, "127881")).toBe(true);
    expect(viewerSpaceCommentReacted(rows, ADA, "128077")).toBe(false);
    expect(viewerSpaceCommentReacted(rows, GUEST, "127881")).toBe(false);
    expect(viewerSpaceCommentReacted(rows, null, "127881")).toBe(false);
    expect(viewerSpaceCommentReacted(rows, undefined, "127881")).toBe(false);
  });
});

describe("comment reaction overlay", () => {
  it("paints a pending toggle over the server rows (SPC-057)", () => {
    const server = [reactionRow({ actor: GUEST, display_name: "Guest" })];
    expect(overlaySpaceCommentReaction(server, { id: ADA, name: "Ada L" }, null)).toEqual(server);
    expect(overlaySpaceCommentReaction(server, null, { action: "add", code: "127881" })).toEqual(server);
    expect(overlaySpaceCommentReaction(server, { id: ADA, name: "Ada L" }, { action: "add", code: "127881" })).toEqual([
      ...server,
      { actor: ADA, reaction: "127881", display_name: "Ada L" },
    ]);
    expect(
      overlaySpaceCommentReaction(
        [...server, reactionRow({ display_name: "Ada L" })],
        { id: ADA, name: "Ada L" },
        { action: "remove", code: "127881" }
      )
    ).toEqual(server);
  });
});

describe("comment image addresses", () => {
  it("resolves confirmed assets through the public anchor route (SPC-053)", () => {
    expect(spaceCommentImageSrc("", "anchor-1", "asset-1")).toBe("/api/public/assets/v2/anchor/anchor-1/asset-1/");
    expect(spaceCommentImageSrc("https://api.test", "anchor-1", "asset-1")).toBe(
      "https://api.test/api/public/assets/v2/anchor/anchor-1/asset-1/"
    );
    expect(spaceCommentImageSrc("https://api.test///", "anchor-1", "asset-1")).toBe(
      "https://api.test/api/public/assets/v2/anchor/anchor-1/asset-1/"
    );
  });
});
