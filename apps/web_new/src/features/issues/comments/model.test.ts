// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { IssueComment } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  applyCommentPatch,
  commentAuthorAvatar,
  commentAuthorName,
  commentBodyImageAssetIds,
  commentDeepLink,
  commentEnterAction,
  commentImageAssetId,
  commentMenuEntries,
  isCommentAuthor,
  isCommentBodyEmpty,
  isCommentFolded,
  patchCommentList,
  rewriteCommentImageSrcs,
  swallowCommentEnter,
  toggledAccess,
  withFold,
} from "./model.js";

const ADA = "523e4567-e89b-12d3-a456-426614174004";
const BOB = "623e4567-e89b-12d3-a456-426614174005";

function comment(overrides: Partial<IssueComment> = {}): IssueComment {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    issue: "223e4567-e89b-12d3-a456-426614174001",
    project: "323e4567-e89b-12d3-a456-426614174002",
    workspace: "423e4567-e89b-12d3-a456-426614174003",
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
    comment_html: "<p>Hello</p>",
    comment_stripped: "Hello",
    comment_json: {},
    attachments: [],
    labels: [],
    access: "INTERNAL",
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

describe("comment authorship", () => {
  it("is author-only, never true for a signed-out viewer", () => {
    expect(isCommentAuthor(comment(), ADA)).toBe(true);
    expect(isCommentAuthor(comment(), BOB)).toBe(false);
    expect(isCommentAuthor(comment(), null)).toBe(false);
  });
});

describe("fold marker", () => {
  it("reads the fold label", () => {
    expect(isCommentFolded(comment())).toBe(false);
    expect(isCommentFolded(comment({ labels: ["fold"] }))).toBe(true);
  });

  it("adds and removes the marker while keeping other labels", () => {
    expect(withFold(comment({ labels: ["pinned"] }), true)).toEqual(["pinned", "fold"]);
    expect(withFold(comment({ labels: ["pinned", "fold"] }), false)).toEqual(["pinned"]);
    expect(withFold(comment({ labels: [] }), false)).toEqual([]);
  });
});

describe("access toggle", () => {
  it("flips internal to external and back", () => {
    expect(toggledAccess(comment())).toBe("EXTERNAL");
    expect(toggledAccess(comment({ access: "EXTERNAL" }))).toBe("INTERNAL");
  });
});

describe("commentMenuEntries", () => {
  const flags = { viewerId: ADA, accessToggle: true, copyLink: true };

  it("gives the author the full menu in order", () => {
    expect(commentMenuEntries(comment(), flags).map((entry) => entry.label)).toEqual([
      "Edit",
      "Copy link",
      "Switch to public comment",
      "Fold comment",
      "Delete",
    ]);
  });

  it("drops edit and delete for another viewer but keeps fold, link and access", () => {
    const labels = commentMenuEntries(comment(), { ...flags, viewerId: BOB }).map((entry) => entry.label);
    expect(labels).toEqual(["Copy link", "Switch to public comment", "Fold comment"]);
  });

  it("always offers fold, even signed out", () => {
    const labels = commentMenuEntries(comment(), { ...flags, viewerId: null, accessToggle: false, copyLink: false });
    expect(labels.map((entry) => entry.label)).toEqual(["Fold comment"]);
  });

  it("relabels access and fold from the comment state", () => {
    const labels = commentMenuEntries(comment({ access: "EXTERNAL", labels: ["fold"] }), flags).map(
      (entry) => entry.label
    );
    expect(labels).toContain("Switch to private comment");
    expect(labels).toContain("Unfold comment");
  });
});

describe("comment header", () => {
  it("names agents by their speaker label with a fallback", () => {
    expect(commentAuthorName(comment({ speaker_type: "agent", speaker_label: "Muse Code" }))).toBe("Muse Code");
    expect(commentAuthorName(comment({ speaker_type: "agent", speaker_label: "  " }))).toBe("AI Agent");
  });

  it("names people from their profile and falls back when unlisted", () => {
    expect(commentAuthorName(comment())).toBe("Ada L");
    expect(commentAuthorName(comment({ actor_detail: null }))).toBe("Unknown author");
    expect(commentAuthorName(comment({ speaker_type: "system", speaker_label: "Ignored", actor_detail: null }))).toBe(
      "Unknown author"
    );
  });

  it("marks bot accounts while a human with bot in their name stays as stored", () => {
    const bot = comment({
      actor_detail: { ...comment().actor_detail!, display_name: "Sync Helper", is_bot: true },
    });
    expect(commentAuthorName(bot)).toBe("Sync Helper Bot");
    expect(commentAuthorName(bot)).toMatch(/bot/i);
    const alreadyMarked = comment({
      actor_detail: { ...comment().actor_detail!, display_name: "Sync Bot", is_bot: true },
    });
    expect(commentAuthorName(alreadyMarked)).toBe("Sync Bot");
    const human = comment({
      actor_detail: { ...comment().actor_detail!, display_name: "Botanist Bo", is_bot: false },
    });
    expect(commentAuthorName(human)).toBe("Botanist Bo");
  });

  it("keeps the agent speaker label even when the actor is a bot account", () => {
    expect(
      commentAuthorName(
        comment({
          speaker_type: "agent",
          speaker_label: "Reviewer",
          actor_detail: { ...comment().actor_detail!, display_name: "Runner", is_bot: true },
        })
      )
    ).toBe("Reviewer");
  });

  it("reads the avatar address only when the server has one", () => {
    expect(commentAuthorAvatar(comment())).toBeUndefined();
    expect(
      commentAuthorAvatar(
        comment({ actor_detail: { ...comment().actor_detail!, avatar_url: "https://cdn.test/a.png" } })
      )
    ).toBe("https://cdn.test/a.png");
  });
});

describe("comment link and empty check", () => {
  it("builds the hash deep link", () => {
    expect(commentDeepLink("https://app.test/acme/browse/WEB-1", "c-1")).toBe(
      "https://app.test/acme/browse/WEB-1#comment-c-1"
    );
  });

  it("treats markup without words as empty", () => {
    expect(isCommentBodyEmpty("<p></p>")).toBe(true);
    expect(isCommentBodyEmpty("<p>   </p>")).toBe(true);
    expect(isCommentBodyEmpty("<p>words</p>")).toBe(false);
  });

  it("counts an image or a mention as content even with no words", () => {
    expect(isCommentBodyEmpty('<p><img src="/api/assets/a-1/" alt=""></p>')).toBe(false);
    expect(isCommentBodyEmpty('<p><image-component src="/a-1/"></image-component></p>')).toBe(false);
    expect(isCommentBodyEmpty('<p><mention-component id="u-1"></mention-component></p>')).toBe(false);
    expect(isCommentBodyEmpty('<p><img src="/a-1/"></p><p></p>')).toBe(false);
  });

  it("leaves other markup-only bodies empty", () => {
    expect(isCommentBodyEmpty("<p><strong> </strong></p>")).toBe(true);
    expect(isCommentBodyEmpty("<p><br></p>")).toBe(true);
  });
});

describe("comment image asset ids (CMT-009)", () => {
  it("reads a bare asset id and the served asset paths", () => {
    expect(commentImageAssetId("asset-9")).toBe("asset-9");
    expect(commentImageAssetId("  asset-9  ")).toBe("asset-9");
    expect(commentImageAssetId("/api/assets/v2/workspaces/acme/projects/p-1/asset-9/")).toBe("asset-9");
    expect(commentImageAssetId("/api/assets/v2/workspaces/acme/projects/p-1/asset-9")).toBe("asset-9");
    expect(commentImageAssetId("https://api.test/api/assets/v2/workspaces/acme/projects/p-1/asset-9/")).toBe("asset-9");
  });

  it("answers null for empty, external and unparseable sources", () => {
    expect(commentImageAssetId("")).toBeNull();
    expect(commentImageAssetId("   ")).toBeNull();
    expect(commentImageAssetId("https://cdn.test/shot.png")).toBeNull();
    expect(commentImageAssetId("data:image/png;base64,AAA")).toBeNull();
    expect(commentImageAssetId("/files/shot.png")).toBeNull();
    expect(commentImageAssetId("not an id")).toBeNull();
    expect(commentImageAssetId("/api/assets/v2/")).toBeNull();
  });

  it("collects embedded ids in order without repeats", () => {
    const html =
      '<p>First <image-component src="asset-1" status="uploaded"></image-component> ' +
      'second <img src="/api/assets/v2/workspaces/acme/projects/p-1/asset-2/"> ' +
      'repeat <image-component src="asset-1" status="uploaded"></image-component> ' +
      'external <img src="https://cdn.test/x.png"></p>';
    expect(commentBodyImageAssetIds(html)).toEqual(["asset-1", "asset-2"]);
    expect(commentBodyImageAssetIds("<p>no pictures</p>")).toEqual([]);
  });

  it("rebinds duplicated ids and leaves everything else alone", () => {
    const html =
      '<p>Keep <image-component src="asset-1" status="uploaded"></image-component> ' +
      'and <img src="/api/assets/v2/workspaces/acme/projects/p-1/asset-2/"></p>';
    const next = rewriteCommentImageSrcs(html, new Map([["asset-1", "asset-1b"]]));
    expect(next).toContain('src="asset-1b"');
    expect(next).not.toContain('asset-1"');
    expect(next).toContain("asset-2");
    expect(next).toContain('status="uploaded"');
  });

  it("returns an unmapped body untouched, never renormalized", () => {
    const html = "<p>Plain <strong>words</strong></p>";
    expect(rewriteCommentImageSrcs(html, new Map())).toBe(html);
    expect(rewriteCommentImageSrcs(html, new Map([["asset-9", "asset-9b"]]))).toBe(html);
  });
});

describe("comment Enter decision (CMT-011)", () => {
  it("picks the mention while the popup is open", () => {
    expect(commentEnterAction({ mentionOpen: true, busy: false, empty: false })).toBe("pick-mention");
    expect(commentEnterAction({ mentionOpen: true, busy: true, empty: true })).toBe("pick-mention");
  });

  it("submits only when idle with content", () => {
    expect(commentEnterAction({ mentionOpen: false, busy: false, empty: false })).toBe("submit");
  });

  it("is a no-op while busy or empty", () => {
    expect(commentEnterAction({ mentionOpen: false, busy: true, empty: false })).toBe("none");
    expect(commentEnterAction({ mentionOpen: false, busy: false, empty: true })).toBe("none");
    expect(commentEnterAction({ mentionOpen: false, busy: true, empty: true })).toBe("none");
  });

  it("swallows every Enter except a submit", () => {
    expect(swallowCommentEnter({ mentionOpen: true, busy: false, empty: false })).toBe(true);
    expect(swallowCommentEnter({ mentionOpen: false, busy: true, empty: false })).toBe(true);
    expect(swallowCommentEnter({ mentionOpen: false, busy: false, empty: true })).toBe(true);
    expect(swallowCommentEnter({ mentionOpen: false, busy: false, empty: false })).toBe(false);
  });
});

describe("comment cache patches", () => {
  it("paints one row and leaves the rest alone", () => {
    const first = comment();
    const second = comment({ id: "923e4567-e89b-12d3-a456-426614174009", labels: ["fold"] });
    const painted = patchCommentList([first, second], first.id, { comment_html: "<p>New</p>" });
    expect(painted[0]?.comment_html).toBe("<p>New</p>");
    expect(painted[1]).toBe(second);
  });

  it("leaves unknown ids and clears via applyCommentPatch", () => {
    const row = comment({ edited_at: "2026-10-17T00:00:00Z" });
    expect(patchCommentList([row], "not-there", { labels: ["fold"] })[0]).toBe(row);
    expect(applyCommentPatch(row, { edited_at: null }).edited_at).toBeNull();
    expect(applyCommentPatch(row, { access: "EXTERNAL" }).access).toBe("EXTERNAL");
  });
});
