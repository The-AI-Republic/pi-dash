// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceComment } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  isSpaceCommentFolded,
  SPACE_COMMENT_FOLD_LABEL,
  SPACE_GENERIC_AGENT_LABEL,
  spaceCommentAuthor,
  spaceCommentBodyHtml,
  spaceCommentSpeakerLabel,
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
